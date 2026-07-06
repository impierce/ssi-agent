use mongo_es::{default_mongo_client, Client, MongoEventRepository, MongoViewRepository};
use shared_kernel::command_handler::{CommandHandler, CommandHandlerFactory, EventUpcaster};
use shared_kernel::cqrs_es::View;
use shared_kernel::cqrs_es::{
    persist::{PersistedEventRepository, PersistedEventStore, PersistenceError, ReplayStream},
    Aggregate, CqrsFramework, Query,
};
use shared_kernel::view_repository::{BoxedViewRepository, ViewRepositoryFactory};
use std::future::Future;
use std::sync::Arc;

pub struct MongoDBStore {
    client: Client,
}

impl MongoDBStore {
    pub async fn new(connection_string: &str) -> Self {
        let client = default_mongo_client(connection_string).await;
        Self { client }
    }
    // TODO: Run [Client::shutdown] during graceful shutdown to close all open connections.

    /// Validates that every persisted event for aggregate `A` can be streamed, upcasted (using
    /// `upcasters`), and deserialized into `A::Event` — i.e. that a full replay of the read side
    /// would succeed with the current code and upcaster configuration.
    ///
    /// This is intended to be run once at startup as a readiness check: it surfaces
    /// upcaster/schema mismatches immediately (before serving traffic), rather than only when a
    /// specific aggregate happens to be loaded later on and its events fail to deserialize.
    ///
    /// Returns the number of events that were successfully validated.
    ///
    /// # Errors
    ///
    /// Returns [`EventValidationError`] if the event repository can't be reached, or if any
    /// persisted event fails to upcast and deserialize into `A::Event`.
    pub async fn validate_events<A>(&self, upcasters: Vec<Box<dyn EventUpcaster>>) -> Result<u64, EventValidationError>
    where
        A: Aggregate,
    {
        let repo = MongoEventRepository::new(self.client.clone())
            .await
            .map_err(|e| EventValidationError {
                aggregate_type: A::TYPE,
                validated_count: 0,
                source: PersistenceError::ConnectionError(Box::new(e)),
            })?;

        let stream = repo
            .stream_all_events::<A>()
            .await
            .map_err(|source| EventValidationError {
                aggregate_type: A::TYPE,
                validated_count: 0,
                source,
            })?;

        validate_event_stream::<A>(stream, &upcasters).await
    }
}

/// Error produced when replaying (streaming + upcasting + deserializing) the persisted events of
/// an aggregate type fails during [`MongoDBStore::validate_events`].
#[derive(Debug, thiserror::Error)]
#[error(
    "event replay validation failed for aggregate type `{aggregate_type}` after successfully \
     validating {validated_count} event(s): {source}"
)]
pub struct EventValidationError {
    /// The aggregate type (`Aggregate::TYPE`) being validated.
    pub aggregate_type: &'static str,
    /// The number of events that were successfully streamed, upcasted, and deserialized before
    /// the failure occurred.
    pub validated_count: u64,
    /// The underlying persistence error (e.g. a deserialization failure caused by a missing
    /// upcaster).
    #[source]
    pub source: PersistenceError,
}

/// Drains `stream`, applying `upcasters` and deserializing each event into `A::Event`, counting
/// the number of events successfully validated.
///
/// This is the testable core of [`MongoDBStore::validate_events`]: it operates on any
/// [`ReplayStream`], so tests can feed it a stream of hand-crafted [`SerializedEvent`]s (via
/// [`ReplayStream::new`] and its accompanying `ReplayFeed`) without a live `MongoDB` instance.
async fn validate_event_stream<A>(
    mut stream: ReplayStream,
    upcasters: &[Box<dyn EventUpcaster>],
) -> Result<u64, EventValidationError>
where
    A: Aggregate,
{
    let mut validated_count = 0u64;
    while let Some(result) = stream.next::<A>(upcasters).await {
        result.map_err(|source| EventValidationError {
            aggregate_type: A::TYPE,
            validated_count,
            source,
        })?;
        validated_count += 1;
    }
    Ok(validated_count)
}

impl ViewRepositoryFactory for MongoDBStore {
    fn create_view_repository<V, A>(&self, name: &str) -> BoxedViewRepository<V, A>
    where
        V: View<A> + Clone + 'static,
        A: Aggregate + 'static,
    {
        BoxedViewRepository::new(Box::new(MongoViewRepository::new(name, self.client.clone())))
    }
}

// TODO: re-expose `mongodb::error::Result` through `mongo_es` and use it as the error type here instead of defining a
// new one.
#[derive(Debug, thiserror::Error)]
#[error("MongoDB aggregate error: {0}")]
pub struct MongoDBAggregateError(String);

impl CommandHandlerFactory for MongoDBStore {
    type Error = MongoDBAggregateError;

    fn create_handler<A>(
        &self,
        services: A::Services,
        queries: Vec<Box<dyn Query<A>>>,
        upcasters: Vec<Box<dyn EventUpcaster>>,
    ) -> impl Future<Output = Result<CommandHandler<A>, Self::Error>> + Send
    where
        A: Aggregate + 'static,
        <A as Aggregate>::Command: Send,
    {
        let client = self.client.clone();

        async move {
            let repo = MongoEventRepository::new(client)
                .await
                .map_err(|e| MongoDBAggregateError(e.to_string()))?;
            let store = PersistedEventStore::new_event_store(repo).with_upcasters(upcasters);

            Ok(Arc::new(CqrsFramework::new(store, queries, services)) as CommandHandler<A>)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use shared_kernel::cqrs_es::event_sink::EventSink;
    use shared_kernel::cqrs_es::persist::{EventUpcaster, SemanticVersionEventUpcaster, SerializedEvent};
    use shared_kernel::cqrs_es::DomainEvent;

    // ── Minimal aggregate/event used to exercise `validate_event_stream` without a live MongoDB ──

    #[derive(Debug, Default, Serialize, Deserialize)]
    struct TestAggregate;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    enum TestEvent {
        SomethingHappened { name: String },
    }

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> String {
            "SomethingHappened".to_string()
        }
        fn event_version(&self) -> String {
            "2".to_string()
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("test aggregate error")]
    struct TestError;

    impl Aggregate for TestAggregate {
        const TYPE: &'static str = "TestAggregate";
        type Command = ();
        type Event = TestEvent;
        type Error = TestError;
        type Services = ();

        async fn handle(
            &mut self,
            _command: Self::Command,
            _services: &Self::Services,
            _sink: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn apply(&mut self, _event: Self::Event) {}
    }

    /// A "legacy" (version "1") shape of `SomethingHappened` that used a `full_name` field
    /// instead of `name`. An upcaster is needed to bring it to the current ("2") shape.
    fn legacy_event(aggregate_id: &str, sequence: usize) -> SerializedEvent {
        SerializedEvent::new(
            aggregate_id.to_string(),
            sequence,
            TestAggregate::TYPE.to_string(),
            "SomethingHappened".to_string(),
            "1".to_string(),
            json!({ "SomethingHappened": { "full_name": "Alice" } }),
            json!({}),
        )
    }

    fn current_event(aggregate_id: &str, sequence: usize) -> SerializedEvent {
        SerializedEvent::new(
            aggregate_id.to_string(),
            sequence,
            TestAggregate::TYPE.to_string(),
            "SomethingHappened".to_string(),
            "2".to_string(),
            json!({ "SomethingHappened": { "name": "Alice" } }),
            json!({}),
        )
    }

    /// Renames the legacy `full_name` field to `name`.
    fn rename_full_name_upcaster() -> Box<dyn EventUpcaster> {
        Box::new(SemanticVersionEventUpcaster::new(
            "SomethingHappened",
            "2",
            Box::new(|payload| {
                let mut payload = payload;
                if let Some(inner) = payload.get_mut("SomethingHappened") {
                    if let Some(full_name) = inner.get_mut("full_name") {
                        let full_name = full_name.take();
                        inner
                            .as_object_mut()
                            .unwrap()
                            .insert("name".to_string(), full_name);
                    }
                }
                payload
            }),
        ))
    }

    /// Builds a `ReplayStream` pre-loaded with `events`, mirroring how `MongoEventRepository`
    /// produces one internally, but without touching `MongoDB`.
    async fn stream_of(events: Vec<SerializedEvent>) -> ReplayStream {
        let (mut feed, stream) = ReplayStream::new(events.len().max(1));
        for event in events {
            feed.push(Ok(event)).await.unwrap();
        }
        drop(feed);
        stream
    }

    #[tokio::test]
    async fn validates_current_shaped_events_without_upcasters() {
        let stream = stream_of(vec![current_event("agg-1", 1), current_event("agg-1", 2)]).await;

        let count = validate_event_stream::<TestAggregate>(stream, &[]).await.unwrap();

        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn legacy_events_fail_validation_without_the_upcaster() {
        let stream = stream_of(vec![legacy_event("agg-1", 1)]).await;

        let error = validate_event_stream::<TestAggregate>(stream, &[]).await.unwrap_err();

        assert_eq!(error.aggregate_type, "TestAggregate");
        assert_eq!(error.validated_count, 0);
        assert!(matches!(error.source, PersistenceError::DeserializationError(_)));
    }

    #[tokio::test]
    async fn legacy_events_pass_validation_with_the_upcaster() {
        let stream = stream_of(vec![legacy_event("agg-1", 1), current_event("agg-1", 2)]).await;
        let upcasters = vec![rename_full_name_upcaster()];

        let count = validate_event_stream::<TestAggregate>(stream, &upcasters).await.unwrap();

        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn reports_the_count_validated_before_a_later_failure() {
        // First event validates fine; second is legacy-shaped without a matching upcaster.
        let stream = stream_of(vec![current_event("agg-1", 1), legacy_event("agg-1", 2)]).await;

        let error = validate_event_stream::<TestAggregate>(stream, &[]).await.unwrap_err();

        assert_eq!(error.validated_count, 1);
    }
}
