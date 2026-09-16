use crate::{
    custom_queries::MutableViewRepository,
    event_verification::{EventVerificationError, RawStoredEvent},
    in_memory::InMemoryViewRepository,
};
use async_trait::async_trait;
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, View};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::Duration,
};

#[derive(Debug)]
pub struct ReplayReport {
    pub aggregate_type: &'static str,
    pub events_replayed: usize,
    pub aggregates_replayed: usize,
    pub duration: Duration,
}

/// Outcome of one replay pass: the projections that were rebuilt, plus the aggregate types that
/// were deliberately left alone.
///
/// An aggregate type with no registered replay job belongs to a downstream crate that owns its own
/// projections, so the core has nothing to rebuild for it. Its events are counted and skipped
/// rather than aborting startup — mirroring how [`crate::event_verification::EventVerifier`] lets
/// downstream crates extend verification instead of failing on unrecognised aggregates.
#[derive(Debug, Default)]
pub struct ReplaySummary {
    pub reports: Vec<ReplayReport>,
    /// Number of events skipped, keyed by aggregate type.
    pub skipped: BTreeMap<String, usize>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error(transparent)]
    EventStore(#[from] EventVerificationError),
    #[error("invalid sequence {sequence} for {aggregate_type}/{aggregate_id}")]
    InvalidSequence {
        aggregate_type: &'static str,
        aggregate_id: String,
        sequence: i64,
    },
    #[error("event sequence for {aggregate_type}/{aggregate_id} is {actual}, expected {expected}")]
    OutOfOrder {
        aggregate_type: &'static str,
        aggregate_id: String,
        expected: usize,
        actual: usize,
    },
    #[error("failed to deserialize {aggregate_type}/{aggregate_id} event {sequence}: {source}")]
    Deserialization {
        aggregate_type: &'static str,
        aggregate_id: String,
        sequence: usize,
        source: serde_json::Error,
    },
    #[error("stored event type `{stored}` does not match deserialized type `{actual}` for {aggregate_type}/{aggregate_id} event {sequence}")]
    EventType {
        aggregate_type: &'static str,
        aggregate_id: String,
        sequence: usize,
        stored: String,
        actual: String,
    },
    #[error("stored event version `{stored}` does not match deserialized version `{actual}` for {aggregate_type}/{aggregate_id} event {sequence}")]
    EventVersion {
        aggregate_type: &'static str,
        aggregate_id: String,
        sequence: usize,
        stored: String,
        actual: String,
    },
    #[error("failed to update replayed projection: {0}")]
    Projection(#[from] cqrs_es::persist::PersistenceError),
}

/// Resolves the replay job for an event's aggregate type.
///
/// Returns `None` for aggregate types with no registered job — those belong to downstream crates
/// that own their own projections — recording the skip in `skipped` so startup can report it.
pub(crate) fn resolve_job<'a>(
    jobs_by_type: &'a HashMap<&'static str, Arc<dyn ReplayJob>>,
    aggregate_type: &str,
    skipped: &mut BTreeMap<String, usize>,
) -> Option<&'a Arc<dyn ReplayJob>> {
    let job = jobs_by_type.get(aggregate_type);
    if job.is_none() {
        *skipped.entry(aggregate_type.to_string()).or_default() += 1;
    }
    job
}

#[async_trait]
pub(crate) trait ReplayJob: Send + Sync {
    fn aggregate_type(&self) -> &'static str;

    async fn apply(&self, raw: RawStoredEvent, sequence: usize) -> Result<(), ReplayError>;
}

pub(crate) struct ReplayProjection<A, V, AV>
where
    A: Aggregate,
    V: View<A>,
    AV: View<A>,
{
    aggregate: Arc<InMemoryViewRepository<V, A>>,
    all_aggregates: Arc<InMemoryViewRepository<AV, A>>,
    all_aggregates_view_id: String,
}

impl<A, V, AV> ReplayProjection<A, V, AV>
where
    A: Aggregate,
    V: View<A>,
    AV: View<A>,
{
    pub(crate) fn new(
        aggregate: Arc<InMemoryViewRepository<V, A>>,
        all_aggregates: Arc<InMemoryViewRepository<AV, A>>,
        all_aggregates_view_id: String,
    ) -> Self {
        Self {
            aggregate,
            all_aggregates,
            all_aggregates_view_id,
        }
    }
}

#[async_trait]
impl<A, V, AV> ReplayJob for ReplayProjection<A, V, AV>
where
    A: Aggregate + 'static,
    V: View<A> + Clone + 'static,
    AV: View<A> + Clone + 'static,
{
    fn aggregate_type(&self) -> &'static str {
        A::TYPE
    }

    async fn apply(&self, raw: RawStoredEvent, sequence: usize) -> Result<(), ReplayError> {
        ReplayProjection::apply(self, raw, sequence).await
    }
}

impl<A, V, AV> ReplayProjection<A, V, AV>
where
    A: Aggregate,
    V: View<A> + Clone,
    AV: View<A> + Clone,
{
    async fn apply(&self, raw: RawStoredEvent, sequence: usize) -> Result<(), ReplayError> {
        let event: A::Event = serde_json::from_value(raw.payload).map_err(|source| ReplayError::Deserialization {
            aggregate_type: A::TYPE,
            aggregate_id: raw.aggregate_id.clone(),
            sequence,
            source,
        })?;
        let actual_type = event.event_type();
        if actual_type != raw.event_type {
            return Err(ReplayError::EventType {
                aggregate_type: A::TYPE,
                aggregate_id: raw.aggregate_id,
                sequence,
                stored: raw.event_type,
                actual: actual_type,
            });
        }
        let actual_version = event.event_version();
        if actual_version != raw.event_version {
            return Err(ReplayError::EventVersion {
                aggregate_type: A::TYPE,
                aggregate_id: raw.aggregate_id,
                sequence,
                stored: raw.event_version,
                actual: actual_version,
            });
        }

        let envelope = EventEnvelope::<A> {
            aggregate_id: raw.aggregate_id.clone(),
            sequence,
            payload: event,
            metadata: HashMap::new(),
        };
        self.aggregate
            .modify(&raw.aggregate_id, &mut |view| view.update(&envelope))
            .await?;
        self.all_aggregates
            .modify(&self.all_aggregates_view_id, &mut |view| view.update(&envelope))
            .await?;
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct ReplayProgress {
    last_sequence: HashMap<String, usize>,
    pending: HashMap<String, BTreeMap<usize, RawStoredEvent>>,
    events_replayed: usize,
    duration: Duration,
}

impl ReplayProgress {
    pub(crate) async fn apply(&mut self, job: &dyn ReplayJob, raw: RawStoredEvent) -> Result<(), ReplayError> {
        let sequence = usize::try_from(raw.sequence).map_err(|_| ReplayError::InvalidSequence {
            aggregate_type: job.aggregate_type(),
            aggregate_id: raw.aggregate_id.clone(),
            sequence: raw.sequence,
        })?;
        let aggregate_id = raw.aggregate_id.clone();
        if self
            .pending
            .entry(aggregate_id.clone())
            .or_default()
            .insert(sequence, raw)
            .is_some()
        {
            let expected = self.last_sequence.get(&aggregate_id).map_or(1, |previous| previous + 1);
            return Err(ReplayError::OutOfOrder {
                aggregate_type: job.aggregate_type(),
                aggregate_id,
                expected,
                actual: sequence,
            });
        }

        loop {
            let expected = self.last_sequence.get(&aggregate_id).map_or(1, |previous| previous + 1);
            let Some(raw) = self
                .pending
                .get_mut(&aggregate_id)
                .and_then(|events| events.remove(&expected))
            else {
                break;
            };
            let started = std::time::Instant::now();
            job.apply(raw, expected).await?;
            self.duration += started.elapsed();
            self.last_sequence.insert(aggregate_id.clone(), expected);
            self.events_replayed += 1;
        }

        Ok(())
    }

    pub(crate) fn finish(self, aggregate_type: &'static str) -> Result<ReplayReport, ReplayError> {
        if let Some((aggregate_id, events)) = self.pending.iter().find(|(_, events)| !events.is_empty()) {
            let expected = self.last_sequence.get(aggregate_id).map_or(1, |previous| previous + 1);
            let actual = *events.first_key_value().expect("pending events is not empty").0;
            return Err(ReplayError::OutOfOrder {
                aggregate_type,
                aggregate_id: aggregate_id.clone(),
                expected,
                actual,
            });
        }

        Ok(ReplayReport {
            aggregate_type,
            events_replayed: self.events_replayed,
            aggregates_replayed: self.last_sequence.len(),
            duration: self.duration,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::{event_sink::EventSink, persist::ViewRepository as _, DomainEvent};
    use serde::{Deserialize, Serialize};
    use std::convert::Infallible;

    #[derive(Default, Deserialize, Serialize)]
    struct TestAggregate;

    #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
    struct TestEvent {
        value: usize,
    }

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> String {
            "test_event".to_string()
        }

        fn event_version(&self) -> String {
            "1".to_string()
        }
    }

    impl Aggregate for TestAggregate {
        const TYPE: &'static str = "replay_test";
        type Command = Infallible;
        type Event = TestEvent;
        type Error = Infallible;
        type Services = ();

        async fn handle(
            &mut self,
            command: Self::Command,
            _service: &Self::Services,
            _sink: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            match command {}
        }

        fn apply(&mut self, _event: Self::Event) {}
    }

    #[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
    struct TestView {
        values: Vec<usize>,
    }

    impl View<TestAggregate> for TestView {
        fn update(&mut self, event: &EventEnvelope<TestAggregate>) {
            self.values.push(event.payload.value);
        }
    }

    fn raw_event(sequence: i64, value: usize) -> RawStoredEvent {
        RawStoredEvent {
            aggregate_type: TestAggregate::TYPE.to_string(),
            aggregate_id: "aggregate-id".to_string(),
            sequence,
            event_type: "test_event".to_string(),
            event_version: "1".to_string(),
            payload: serde_json::to_value(TestEvent { value }).unwrap(),
        }
    }

    #[test]
    fn events_of_an_unregistered_aggregate_are_skipped_and_counted() {
        // Downstream crates (e.g. ssi-agent-ext's `iam`) own aggregates the core does not project.
        // Their events must not abort startup.
        let jobs: HashMap<&'static str, Arc<dyn ReplayJob>> = HashMap::new();
        let mut skipped = BTreeMap::new();

        assert!(resolve_job(&jobs, "iam", &mut skipped).is_none());
        assert!(resolve_job(&jobs, "iam", &mut skipped).is_none());
        assert!(resolve_job(&jobs, "other", &mut skipped).is_none());

        assert_eq!(skipped.get("iam"), Some(&2));
        assert_eq!(skipped.get("other"), Some(&1));
    }

    #[tokio::test]
    async fn replay_buffers_out_of_order_object_ids_and_materializes_both_views() {
        let aggregate = Arc::new(InMemoryViewRepository::default());
        let all_aggregates = Arc::new(InMemoryViewRepository::default());
        let projection = ReplayProjection::<TestAggregate, TestView, TestView>::new(
            aggregate.clone(),
            all_aggregates.clone(),
            "all_replay_tests".to_string(),
        );
        let mut progress = ReplayProgress::default();

        progress.apply(&projection, raw_event(2, 2)).await.unwrap();
        assert!(aggregate.load("aggregate-id").await.unwrap().is_none());

        progress.apply(&projection, raw_event(1, 1)).await.unwrap();
        let report = progress.finish(TestAggregate::TYPE).unwrap();

        assert_eq!(report.events_replayed, 2);
        assert_eq!(report.aggregates_replayed, 1);
        assert_eq!(
            aggregate.load("aggregate-id").await.unwrap().unwrap().values,
            vec![1, 2]
        );
        assert_eq!(
            all_aggregates.load("all_replay_tests").await.unwrap().unwrap().values,
            vec![1, 2]
        );
    }
}
