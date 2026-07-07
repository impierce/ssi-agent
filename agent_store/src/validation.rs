use cqrs_es::persist::{EventUpcaster, PersistenceError, ReplayStream};
use cqrs_es::Aggregate;

/// Error produced when replaying (streaming + upcasting + deserializing) the persisted events of
/// an aggregate type fails during startup replay validation (see [`crate::CqrsComponentBuilder::validate_events`]
/// and [`crate::validate_all_events`]).
///
/// This is intentionally *not* fatal to the running process: the application is expected to log
/// this loudly, mark itself not-ready (so `/readyz` returns `503`), and keep serving traffic so an
/// orchestrator can hold back the old revision instead of losing the entire deployment.
#[derive(Debug, thiserror::Error)]
#[error(
    "event replay validation failed for aggregate type `{aggregate_type}` after successfully \
     validating {validated_count} event(s): {source}"
)]
pub struct EventValidationError {
    /// The aggregate type (`Aggregate::TYPE`) being validated.
    pub aggregate_type: &'static str,
    /// The number of events that were successfully streamed, upcasted, and deserialized before
    /// the failure occurred (across the whole validation sweep, not just this aggregate type).
    pub validated_count: u64,
    /// The underlying persistence error (e.g. a deserialization failure caused by a missing
    /// upcaster).
    #[source]
    pub source: PersistenceError,
}

/// Drains `stream`, applying `upcasters` and deserializing each event into `A::Event`, counting
/// the number of events successfully validated.
///
/// This is the testable core of the per-backend `CqrsComponentBuilder::validate_events`
/// implementations: it operates on any [`ReplayStream`], so tests can feed it a stream of
/// hand-crafted `SerializedEvent`s (via `ReplayStream::new` and its accompanying `ReplayFeed`)
/// without a live database.
pub async fn validate_event_stream<A>(
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

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::event_sink::EventSink;
    use cqrs_es::persist::{SemanticVersionEventUpcaster, SerializedEvent};
    use cqrs_es::DomainEvent;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    // ── Minimal aggregate/event used to exercise `validate_event_stream` without a live database ──

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
                        inner.as_object_mut().unwrap().insert("name".to_string(), full_name);
                    }
                }
                payload
            }),
        ))
    }

    /// Builds a `ReplayStream` pre-loaded with `events`, mirroring how a `PersistedEventRepository`
    /// produces one internally, but without touching a live database.
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

        let count = validate_event_stream::<TestAggregate>(stream, &upcasters)
            .await
            .unwrap();

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
