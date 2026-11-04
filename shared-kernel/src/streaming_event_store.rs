//! An event store that loads aggregates by applying their events while they are read.
//!
//! # Why this exists
//!
//! `cqrs-es` 0.5 loads an aggregate in [`PersistedEventStore::load_aggregate`] through
//! [`PersistedEventRepository::get_events`]: all events of the aggregate are read into a `Vec` of
//! JSON payloads, deserialized into a second `Vec`, and only then applied. Loading an aggregate thus
//! needs memory proportional to its whole history. For aggregates with many large events this is
//! the dominant memory cost of the agent: every server config event carries all credential
//! configurations, and with a few hundred of them a single command allocated about 250 MiB, enough
//! to get a container with a 256 MiB limit killed.
//!
//! The repository trait already offers a bounded stream of events
//! ([`PersistedEventRepository::stream_events`]), but `cqrs-es` only uses it to replay queries.
//! [`StreamingEventStore`] uses it for loading aggregates and leaves everything else to
//! [`PersistedEventStore`], so `cqrs-es` itself is used unmodified.
//!
//! # Limits
//!
//! - No snapshots: snapshot loading would need a stream starting after the snapshot's sequence,
//!   which the repository trait does not offer.
//! - No event upcasters, as none are used.

use cqrs_es::persist::{EventStoreAggregateContext, PersistedEventRepository, PersistedEventStore};
use cqrs_es::{Aggregate, AggregateError, EventEnvelope, EventStore};
use std::collections::HashMap;

/// Events a replay repository should buffer ahead of the aggregate applying them.
pub const REPLAY_BUFFER_EVENTS: usize = 4;

/// Wraps a [`PersistedEventStore`] (without snapshots) and replaces its aggregate loading with a
/// streamed replay, so at most a few events are held in memory at a time. Loading events and
/// committing are delegated unchanged.
pub struct StreamingEventStore<R, A>
where
    R: PersistedEventRepository,
    A: Aggregate,
{
    store: PersistedEventStore<R, A>,
    replay: R,
}

impl<R, A> StreamingEventStore<R, A>
where
    R: PersistedEventRepository,
    A: Aggregate,
{
    /// Builds the store from two repositories on the same storage: one backs the wrapped
    /// [`PersistedEventStore`], the other streams events when an aggregate is loaded. Two are
    /// needed because the wrapped store takes ownership of its repository, and because the replay
    /// repository should buffer only a few events (see [`REPLAY_BUFFER_EVENTS`]) while the stream
    /// size of the other one stays as configured for query replays.
    pub fn new(store_repository: R, replay_repository: R) -> Self {
        Self {
            store: PersistedEventStore::new_event_store(store_repository),
            replay: replay_repository,
        }
    }
}

impl<R, A> EventStore<A> for StreamingEventStore<R, A>
where
    R: PersistedEventRepository,
    A: Aggregate + Send + Sync,
{
    type AC = EventStoreAggregateContext<A>;

    async fn load_events(&self, aggregate_id: &str) -> Result<Vec<EventEnvelope<A>>, AggregateError<A::Error>> {
        self.store.load_events(aggregate_id).await
    }

    async fn load_aggregate(&self, aggregate_id: &str) -> Result<Self::AC, AggregateError<A::Error>> {
        let mut context = EventStoreAggregateContext {
            aggregate_id: aggregate_id.to_string(),
            aggregate: A::default(),
            current_sequence: 0,
            current_snapshot: None,
        };

        let mut events = self.replay.stream_events::<A>(aggregate_id).await?;
        while let Some(envelope) = events.next::<A>(&[]).await {
            let envelope = envelope?;
            // A replay stream may skip events it fails to read (`mongo-es` 0.4.1 logs and drops
            // them, where `get_events` returns an error), which would leave the aggregate in a
            // wrong state without any error.
            let expected = context.current_sequence + 1;
            if envelope.sequence != expected {
                return Err(AggregateError::UnexpectedError(
                    format!(
                        "incomplete event stream for `{aggregate_id}`: expected sequence {expected}, got {}",
                        envelope.sequence
                    )
                    .into(),
                ));
            }
            context.current_sequence = envelope.sequence;
            context.aggregate.apply(envelope.payload);
        }

        Ok(context)
    }

    async fn commit(
        &self,
        events: Vec<A::Event>,
        context: Self::AC,
        metadata: HashMap<String, String>,
    ) -> Result<Vec<EventEnvelope<A>>, AggregateError<A::Error>> {
        self.store.commit(events, context, metadata).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::persist::{PersistenceError, ReplayStream, SerializedEvent, SerializedSnapshot};
    use cqrs_es::{event_sink::EventSink, DomainEvent};
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Default, Serialize, Deserialize)]
    struct Counter {
        total: i64,
        applied: Vec<i64>,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Added(i64);

    impl DomainEvent for Added {
        fn event_type(&self) -> String {
            "Added".to_string()
        }

        fn event_version(&self) -> String {
            "1".to_string()
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("counter error")]
    struct CounterError;

    impl Aggregate for Counter {
        type Command = i64;
        type Event = Added;
        type Error = CounterError;
        type Services = ();

        const TYPE: &'static str = "counter";

        async fn handle(
            &mut self,
            command: Self::Command,
            _services: &Self::Services,
            sink: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            sink.write(Added(command), self).await;
            Ok(())
        }

        fn apply(&mut self, Added(value): Self::Event) {
            self.total += value;
            self.applied.push(value);
        }
    }

    /// Stores serialized events in memory; `get_events` is unused by the streamed load and fails.
    #[derive(Clone, Default)]
    struct MemoryRepository(Arc<Mutex<Vec<SerializedEvent>>>);

    impl PersistedEventRepository for MemoryRepository {
        async fn get_events<A: Aggregate>(
            &self,
            _aggregate_id: &str,
        ) -> Result<Vec<SerializedEvent>, PersistenceError> {
            Err(PersistenceError::UnknownError("full load used".into()))
        }

        async fn get_last_events<A: Aggregate>(
            &self,
            _aggregate_id: &str,
            _last_sequence: usize,
        ) -> Result<Vec<SerializedEvent>, PersistenceError> {
            Err(PersistenceError::UnknownError("full load used".into()))
        }

        async fn get_snapshot<A: Aggregate>(
            &self,
            _aggregate_id: &str,
        ) -> Result<Option<SerializedSnapshot>, PersistenceError> {
            Ok(None)
        }

        async fn persist<A: Aggregate>(
            &self,
            events: &[SerializedEvent],
            _snapshot_update: Option<(String, serde_json::Value, usize)>,
        ) -> Result<(), PersistenceError> {
            self.0.lock().unwrap().extend_from_slice(events);
            Ok(())
        }

        async fn stream_events<A: Aggregate>(&self, aggregate_id: &str) -> Result<ReplayStream, PersistenceError> {
            let events: Vec<_> = self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.aggregate_id == aggregate_id)
                .cloned()
                .collect();
            let (mut feed, stream) = ReplayStream::new(1);
            tokio::spawn(async move {
                for event in events {
                    if feed.push(Ok(event)).await.is_err() {
                        return;
                    }
                }
            });
            Ok(stream)
        }

        async fn stream_all_events<A: Aggregate>(&self) -> Result<ReplayStream, PersistenceError> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn loads_aggregates_by_streaming_their_events_in_order() {
        let repository = MemoryRepository::default();
        let store = StreamingEventStore::<_, Counter>::new(repository.clone(), repository.clone());

        for value in [3, 4, 5] {
            let context = store.load_aggregate("a").await.unwrap();
            store.commit(vec![Added(value)], context, HashMap::new()).await.unwrap();
        }
        let other = store.load_aggregate("b").await.unwrap();
        store.commit(vec![Added(100)], other, HashMap::new()).await.unwrap();

        let context = store.load_aggregate("a").await.unwrap();
        assert_eq!(context.current_sequence, 3);
        assert_eq!(context.aggregate.applied, vec![3, 4, 5]);
        assert_eq!(context.aggregate.total, 12);
        assert_eq!(repository.0.lock().unwrap()[3].payload, json!(100));
    }

    #[tokio::test]
    async fn rejects_an_event_stream_with_a_gap() {
        let repository = MemoryRepository::default();
        let store = StreamingEventStore::<_, Counter>::new(repository.clone(), repository.clone());
        for value in [1, 2, 3] {
            let context = store.load_aggregate("a").await.unwrap();
            store.commit(vec![Added(value)], context, HashMap::new()).await.unwrap();
        }
        repository.0.lock().unwrap().remove(1);

        assert!(matches!(
            store.load_aggregate("a").await,
            Err(AggregateError::UnexpectedError(_))
        ));
    }
}
