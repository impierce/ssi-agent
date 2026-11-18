//! Rebuilds in-memory views from the stored events at startup.
//!
//! Each aggregate type registers a [`ReplayJob`] for its two views (one per aggregate, one listing
//! all aggregates). Replay applies stored events to these views directly: no query of the CQRS
//! pipeline runs, so nothing is published and no command is executed again.
//!
//! Events are expected in insertion order per aggregate type. Within an aggregate, an event whose
//! predecessor has not been read yet is held back until it arrives. An event that cannot be applied
//! (unknown payload, missing predecessor) is logged and counted; the persisted event verification
//! reports it as well and keeps the application unready.

use crate::in_memory::InMemoryViewRepository;
use async_trait::async_trait;
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, View};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::error;

/// An event as stored, before deserialization of its payload.
#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub event_version: String,
    pub payload: serde_json::Value,
    pub metadata: HashMap<String, String>,
}

/// Views rebuilt for one aggregate type.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AggregateTypeReplay {
    pub events: usize,
    pub aggregates: usize,
    /// Events not applied: unknown payload, or a predecessor missing.
    pub rejected: usize,
}

#[derive(Debug, Default)]
pub struct ReplaySummary {
    pub replayed: BTreeMap<&'static str, AggregateTypeReplay>,
    /// Events of aggregate types without views here (e.g. owned by an extension, or a feature that
    /// is switched off), by aggregate type.
    pub skipped: BTreeMap<String, usize>,
    pub duration: Duration,
}

impl ReplaySummary {
    pub fn events(&self) -> usize {
        self.replayed.values().map(|replay| replay.events).sum()
    }

    pub fn rejected(&self) -> usize {
        self.replayed.values().map(|replay| replay.rejected).sum()
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RejectedEvent {
    #[error("payload does not deserialize: {0}")]
    Payload(serde_json::Error),
    #[error("stored as `{stored}`, deserialized as `{actual}`")]
    EventType { stored: String, actual: String },
    #[error("stored with version `{stored}`, deserialized with version `{actual}`")]
    EventVersion { stored: String, actual: String },
    #[error("invalid sequence")]
    Sequence,
}

#[async_trait]
pub(crate) trait ReplayJob: Send + Sync {
    fn aggregate_type(&self) -> &'static str;

    async fn apply(&self, event: StoredEvent, sequence: usize) -> Result<(), RejectedEvent>;
}

/// Applies an aggregate type's events to its two in-memory views.
pub(crate) struct ReplayProjection<A: Aggregate, V: View<A>, AV: View<A>> {
    aggregate: Arc<InMemoryViewRepository<V, A>>,
    all_aggregates: Arc<InMemoryViewRepository<AV, A>>,
    all_aggregates_name: String,
}

impl<A: Aggregate, V: View<A>, AV: View<A>> ReplayProjection<A, V, AV> {
    pub(crate) fn new(
        aggregate: Arc<InMemoryViewRepository<V, A>>,
        all_aggregates: Arc<InMemoryViewRepository<AV, A>>,
        all_aggregates_name: &str,
    ) -> Self {
        Self {
            aggregate,
            all_aggregates,
            all_aggregates_name: all_aggregates_name.to_string(),
        }
    }
}

#[async_trait]
impl<A, V, AV> ReplayJob for ReplayProjection<A, V, AV>
where
    A: Aggregate + 'static,
    V: View<A> + 'static,
    AV: View<A> + 'static,
{
    fn aggregate_type(&self) -> &'static str {
        A::TYPE
    }

    async fn apply(&self, event: StoredEvent, sequence: usize) -> Result<(), RejectedEvent> {
        let payload: A::Event = serde_json::from_value(event.payload).map_err(RejectedEvent::Payload)?;
        if payload.event_type() != event.event_type {
            return Err(RejectedEvent::EventType {
                stored: event.event_type,
                actual: payload.event_type(),
            });
        }
        if payload.event_version() != event.event_version {
            return Err(RejectedEvent::EventVersion {
                stored: event.event_version,
                actual: payload.event_version(),
            });
        }

        let envelope = [EventEnvelope::<A> {
            aggregate_id: event.aggregate_id,
            sequence,
            payload,
            metadata: event.metadata,
        }];
        self.aggregate.apply(&envelope[0].aggregate_id, &envelope).await;
        self.all_aggregates.apply(&self.all_aggregates_name, &envelope).await;
        Ok(())
    }
}

/// Replay of the stored events, fed one event at a time.
pub(crate) struct Replay {
    jobs: HashMap<&'static str, Arc<dyn ReplayJob>>,
    /// Last applied sequence, and events held back until their predecessor is applied, per
    /// aggregate type and aggregate.
    last_sequences: HashMap<(&'static str, String), usize>,
    held_back: HashMap<(&'static str, String), BTreeMap<usize, StoredEvent>>,
    summary: ReplaySummary,
    started: Instant,
}

impl Replay {
    pub(crate) fn new(jobs: &[Arc<dyn ReplayJob>]) -> Self {
        Self {
            jobs: jobs.iter().map(|job| (job.aggregate_type(), job.clone())).collect(),
            last_sequences: HashMap::new(),
            held_back: HashMap::new(),
            summary: ReplaySummary {
                replayed: jobs
                    .iter()
                    .map(|job| (job.aggregate_type(), AggregateTypeReplay::default()))
                    .collect(),
                ..ReplaySummary::default()
            },
            started: Instant::now(),
        }
    }

    pub(crate) async fn apply(&mut self, event: StoredEvent) {
        let Some(job) = self.jobs.get(event.aggregate_type.as_str()).cloned() else {
            *self.summary.skipped.entry(event.aggregate_type).or_default() += 1;
            return;
        };
        let aggregate_type = job.aggregate_type();

        let Some(sequence) = usize::try_from(event.sequence).ok().filter(|sequence| *sequence > 0) else {
            self.reject(&event, &RejectedEvent::Sequence);
            return;
        };
        let key = (aggregate_type, event.aggregate_id.clone());
        let held_back = self.held_back.entry(key.clone()).or_default();
        if held_back.insert(sequence, event).is_some() {
            if let Some(event) = held_back.get(&sequence).cloned() {
                self.reject(&event, &RejectedEvent::Sequence);
            }
            return;
        }

        // Apply the held back events that are next in sequence.
        loop {
            let next = self.last_sequences.get(&key).map_or(1, |last| last + 1);
            let Some(event) = self
                .held_back
                .get_mut(&key)
                .and_then(|held_back| held_back.remove(&next))
            else {
                break;
            };
            match job.apply(event.clone(), next).await {
                Ok(()) => {
                    let replay = self.summary.replayed.entry(aggregate_type).or_default();
                    replay.events += 1;
                    if next == 1 {
                        replay.aggregates += 1;
                    }
                }
                Err(rejected) => self.reject(&event, &rejected),
            }
            // A rejected event still counts as applied, so its successors are not held back too.
            self.last_sequences.insert(key.clone(), next);
        }
        if self.held_back.get(&key).is_some_and(BTreeMap::is_empty) {
            self.held_back.remove(&key);
        }
    }

    fn reject(&mut self, event: &StoredEvent, rejected: &RejectedEvent) {
        error!(
            aggregate_type = %event.aggregate_type,
            aggregate_id = %event.aggregate_id,
            sequence = event.sequence,
            event_type = %event.event_type,
            reason = %rejected,
            "Stored event not replayed"
        );
        if let Some(replay) = self
            .jobs
            .get(event.aggregate_type.as_str())
            .and_then(|job| self.summary.replayed.get_mut(job.aggregate_type()))
        {
            replay.rejected += 1;
        }
    }

    pub(crate) fn finish(mut self) -> ReplaySummary {
        let held_back: Vec<StoredEvent> = std::mem::take(&mut self.held_back)
            .into_values()
            .flat_map(BTreeMap::into_values)
            .collect();
        for event in held_back {
            // Its predecessor is missing.
            self.reject(&event, &RejectedEvent::Sequence);
        }
        self.summary.duration = self.started.elapsed();
        self.summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::{event_sink::EventSink, persist::ViewRepository as _};
    use serde::{Deserialize, Serialize};
    use std::convert::Infallible;

    #[derive(Default, Deserialize, Serialize)]
    struct TestAggregate;

    #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
    struct Added(usize);

    impl DomainEvent for Added {
        fn event_type(&self) -> String {
            "Added".to_string()
        }

        fn event_version(&self) -> String {
            "1".to_string()
        }
    }

    impl Aggregate for TestAggregate {
        const TYPE: &'static str = "replay_test";
        type Command = Infallible;
        type Event = Added;
        type Error = Infallible;
        type Services = ();

        async fn handle(
            &mut self,
            command: Self::Command,
            _: &Self::Services,
            _: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            match command {}
        }

        fn apply(&mut self, _event: Self::Event) {}
    }

    /// Values in order of application, and the timestamp of the last event.
    #[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
    struct Values(Vec<usize>, Option<String>);

    impl View<TestAggregate> for Values {
        fn update(&mut self, event: &EventEnvelope<TestAggregate>) {
            self.0.push(event.payload.0);
            self.1 = event.metadata.get("timestamp").cloned();
        }
    }

    type Views = (
        Arc<InMemoryViewRepository<Values, TestAggregate>>,
        Arc<InMemoryViewRepository<Values, TestAggregate>>,
    );

    fn replay() -> (Replay, Views) {
        let aggregate = Arc::new(InMemoryViewRepository::default());
        let all_aggregates = Arc::new(InMemoryViewRepository::default());
        let job: Arc<dyn ReplayJob> = Arc::new(ReplayProjection::new(
            aggregate.clone(),
            all_aggregates.clone(),
            "all_replay_tests",
        ));
        (Replay::new(&[job]), (aggregate, all_aggregates))
    }

    fn stored(aggregate_id: &str, sequence: i64, value: usize) -> StoredEvent {
        StoredEvent {
            aggregate_type: TestAggregate::TYPE.to_string(),
            aggregate_id: aggregate_id.to_string(),
            sequence,
            event_type: "Added".to_string(),
            event_version: "1".to_string(),
            payload: serde_json::to_value(Added(value)).unwrap(),
            metadata: HashMap::from([("timestamp".to_string(), format!("t{value}"))]),
        }
    }

    #[tokio::test]
    async fn rebuilds_both_views_in_stored_order_with_metadata() {
        let (mut replay, (aggregate, all_aggregates)) = replay();
        for event in [stored("a", 1, 1), stored("b", 1, 2), stored("a", 2, 3)] {
            replay.apply(event).await;
        }
        let summary = replay.finish();

        assert_eq!(
            aggregate.load("a").await.unwrap(),
            Some(Values(vec![1, 3], Some("t3".to_string())))
        );
        assert_eq!(
            aggregate.load("b").await.unwrap(),
            Some(Values(vec![2], Some("t2".to_string())))
        );
        assert_eq!(
            all_aggregates.load("all_replay_tests").await.unwrap().unwrap().0,
            vec![1, 2, 3]
        );
        assert_eq!(
            summary.replayed[TestAggregate::TYPE],
            AggregateTypeReplay {
                events: 3,
                aggregates: 2,
                rejected: 0
            }
        );
    }

    #[tokio::test]
    async fn holds_back_an_event_until_its_predecessor() {
        let (mut replay, (aggregate, _)) = replay();
        replay.apply(stored("a", 2, 2)).await;
        assert_eq!(aggregate.load("a").await.unwrap(), None);
        replay.apply(stored("a", 1, 1)).await;

        assert_eq!(aggregate.load("a").await.unwrap().unwrap().0, vec![1, 2]);
        assert_eq!(replay.finish().rejected(), 0);
    }

    #[tokio::test]
    async fn rejects_events_without_predecessor_or_with_unknown_payload() {
        let (mut replay, (aggregate, _)) = replay();
        replay.apply(stored("a", 1, 1)).await;
        replay.apply(stored("a", 3, 3)).await;
        replay
            .apply(StoredEvent {
                payload: serde_json::json!({ "unknown": true }),
                ..stored("b", 1, 0)
            })
            .await;
        replay.apply(stored("b", 2, 2)).await;
        let summary = replay.finish();

        assert_eq!(aggregate.load("a").await.unwrap().unwrap().0, vec![1]);
        assert_eq!(aggregate.load("b").await.unwrap().unwrap().0, vec![2]);
        assert_eq!(summary.rejected(), 2);
    }

    #[tokio::test]
    async fn skips_aggregate_types_without_views() {
        let (mut replay, _) = replay();
        replay
            .apply(StoredEvent {
                aggregate_type: "extension".to_string(),
                ..stored("a", 1, 1)
            })
            .await;
        let summary = replay.finish();

        assert_eq!(summary.skipped.get("extension"), Some(&1));
        assert_eq!(summary.events(), 0);
    }
}
