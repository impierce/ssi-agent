use async_trait::async_trait;
use chrono::{DateTime, Utc};
use convert_case::{Case, Casing};
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, Query};
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;

#[allow(clippy::doc_markdown)]
/// A CNCF CloudEvent envelope (v1.0 spec).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub struct CloudEvent {
    pub id: String,
    pub source: String,
    pub specversion: String,
    #[serde(rename = "type")]
    #[schema(example = "com.impierce.unicore.offer-created")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datacontenttype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataschema: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl CloudEvent {
    pub fn new(event_type: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source: source.into(),
            specversion: "1.0".to_string(),
            event_type: event_type.into(),
            datacontenttype: Some("application/json".to_string()),
            dataschema: None,
            subject: None,
            time: Some(Utc::now()),
            data: None,
        }
    }

    #[must_use]
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }

    #[must_use]
    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }
}

/// Helper function to construct a reverse-DNS standard CNCF `CloudEvent` v1.0.
#[must_use]
pub fn build_cloud_event(
    aggregate_type: &str,
    aggregate_id: &str,
    sequence: usize,
    event_type: &str,
    payload: serde_json::Value,
    occurred_at: Option<DateTime<Utc>>,
) -> CloudEvent {
    let cloud_type = format!("com.impierce.unicore.{}", event_type.to_case(Case::Kebab));
    let source = format!("/services/{}", aggregate_type.to_lowercase());
    let id = format!("{aggregate_type}:{aggregate_id}:{sequence}");

    // TODO: Manually unwrapping enum variant tags from payloads until enum serialization is refactored (e.g., via adjacent tagging).
    let data = payload.get(event_type).cloned().unwrap_or(payload);

    CloudEvent {
        id,
        source,
        specversion: "1.0".to_string(),
        event_type: cloud_type,
        datacontenttype: Some("application/json".to_string()),
        dataschema: None,
        subject: Some(aggregate_id.to_string()),
        time: occurred_at.or_else(|| Some(Utc::now())),
        data: Some(data),
    }
}

/// Criteria for filtering events on the event bus.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventFilter {
    pub event_types: Vec<String>,
    pub sources: Vec<String>,
    pub subject: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}

impl EventFilter {
    #[must_use]
    pub fn matches(&self, event: &CloudEvent) -> bool {
        if !self.event_types.is_empty()
            && !self.event_types.iter().any(|event_type| {
                event_type.eq_ignore_ascii_case(&event.event_type)
                    || event.event_type.to_lowercase().contains(&event_type.to_lowercase())
            })
        {
            return false;
        }
        if !self.sources.is_empty()
            && !self.sources.iter().any(|source_pattern| {
                source_pattern.eq_ignore_ascii_case(&event.source)
                    || event.source.to_lowercase().contains(&source_pattern.to_lowercase())
            })
        {
            return false;
        }
        if let Some(ref target_subject) = self.subject {
            if event.subject.as_ref() != Some(target_subject) {
                return false;
            }
        }
        if let Some(since) = self.since {
            if let Some(event_time) = event.time {
                if event_time < since {
                    return false;
                }
            }
        }
        if let Some(until) = self.until {
            if let Some(event_time) = event.time {
                if event_time > until {
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Error, Debug, Clone)]
pub enum EventBusError {
    #[error("Subscriber lagged behind by {0} events")]
    Lagged(u64),
    #[error("Event source error: {0}")]
    Source(String),
    #[error("Position-based subscription is unsupported")]
    UnsupportedPosition,
    #[error("Event bus stream closed")]
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position(pub Vec<u8>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscribePosition {
    Live,
    From(Position),
}

/// An item yielded by an [`EventSourceStream`], containing the [`CloudEvent`] and an optional [`Position`] for resuming.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceEvent {
    pub event: CloudEvent,
    pub position: Option<Position>,
}

impl SourceEvent {
    #[must_use]
    pub fn new(event: CloudEvent, position: Option<Position>) -> Self {
        Self { event, position }
    }
}

pub type BusEventStream = Pin<Box<dyn Stream<Item = Result<CloudEvent, EventBusError>> + Send>>;
pub type EventSourceStream = Pin<Box<dyn Stream<Item = Result<SourceEvent, EventBusError>> + Send>>;

/// Port for subscribing to the internal event bus.
pub trait EventBus: Send + Sync {
    fn subscribe(&self, filter: EventFilter) -> BusEventStream;
}

/// SPI for event source adapters (e.g., `MongoDB` Change Streams).
#[async_trait]
pub trait EventSource: Send + Sync + 'static {
    async fn open(&self, from: SubscribePosition) -> Result<EventSourceStream, EventBusError>;
}

/// SPI for reading historical events from persistent storage.
#[async_trait]
pub trait EventHistoryReader: Send + Sync + 'static {
    async fn history_ascending(
        &self,
        filter: &EventFilter,
        last_event_id: Option<&str>,
        limit: Option<usize>,
    ) -> Result<HistoryAscendingResult, EventBusError>;
}

use std::collections::VecDeque;

/// In-process event bus handle backed by Tokio broadcast channel and recent event history ring-buffer.
#[derive(Clone)]
pub struct EventBusHandle {
    sender: tokio::sync::broadcast::Sender<Arc<CloudEvent>>,
    history: Arc<std::sync::RwLock<VecDeque<CloudEvent>>>,
    history_capacity: usize,
    history_reader: Arc<std::sync::RwLock<Option<Arc<dyn EventHistoryReader>>>>,
}

impl EventBusHandle {
    /// Creates a new `EventBusHandle` with the specified broadcast channel `capacity`.
    ///
    /// The in-memory history ring-buffer is allocated for 500 events, providing a reasonable
    /// window for subscriber catch-up (e.g. SSE reconnects via `Last-Event-ID`) while bounding memory usage.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(capacity);
        Self {
            sender,
            history: Arc::new(std::sync::RwLock::new(VecDeque::with_capacity(500))),
            history_capacity: 500,
            history_reader: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Sets the persistent history reader SPI implementation.
    pub fn set_history_reader(&self, reader: Arc<dyn EventHistoryReader>) {
        let mut lock = match self.history_reader.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *lock = Some(reader);
    }
}

impl Default for EventBusHandle {
    fn default() -> Self {
        Self::new(1024)
    }
}

/// Result of querying historical events in ascending order.
///
/// Contains the list of matching [`CloudEvent`]s and a `gap_detected` flag indicating
/// whether a requested `last_event_id` was absent from memory/storage (e.g. evicted ring-buffer).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HistoryAscendingResult {
    pub events: Vec<CloudEvent>,
    /// Set to `true` if `last_event_id` was specified but could not be found in retained history.
    pub gap_detected: bool,
}

impl EventBusHandle {
    /// Publishes a [`CloudEvent`] to all active subscribers and synchronously appends it to the in-memory ring-buffer.
    ///
    /// If an event with the same ID is already present in the history buffer, it is dropped as a duplicate.
    pub fn publish(&self, event: CloudEvent) {
        let mut lock = match self.history.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        if lock.iter().rev().any(|existing_event| existing_event.id == event.id) {
            return;
        }

        if lock.len() >= self.history_capacity {
            lock.pop_front();
        }
        lock.push_back(event.clone());
        drop(lock);

        let _ = self.sender.send(Arc::new(event));
    }

    /// Queries the most recent historical events matching `filter` in descending order up to `limit`.
    #[must_use]
    pub fn history(&self, filter: &EventFilter, limit: usize) -> Vec<CloudEvent> {
        let lock = match self.history.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        lock.iter()
            .rev()
            .filter(|event| filter.matches(event))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Returns historical events in chronological (ascending) order matching `filter`.
    ///
    /// If `last_event_id` is specified:
    /// - If found, events occurring *after* `last_event_id` are returned (`gap_detected = false`).
    /// - If missing/evicted, the latest `limit` events are returned and `gap_detected` is set to `true`.
    ///
    /// When `limit` is `None`, all matching events are returned without truncation.
    #[must_use]
    pub async fn history_ascending(
        &self,
        filter: &EventFilter,
        last_event_id: Option<&str>,
        limit: Option<usize>,
    ) -> HistoryAscendingResult {
        let reader = {
            let lock = match self.history_reader.read() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            lock.clone()
        };

        if let Some(reader) = reader {
            match reader.history_ascending(filter, last_event_id, limit).await {
                Ok(result) => return result,
                Err(err) => {
                    tracing::error!(
                        "EventHistoryReader failed, falling back to in-memory history: {:?}",
                        err
                    );
                }
            }
        }

        let lock = match self.history.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let mut gap_detected = false;

        let events: Vec<CloudEvent> = if let Some(last_id) = last_event_id {
            if let Some(pos) = lock.iter().position(|event| event.id == last_id) {
                let iter = lock.iter().skip(pos + 1).filter(|event| filter.matches(event));
                match limit {
                    Some(max_count) => iter.take(max_count).cloned().collect(),
                    None => iter.cloned().collect(),
                }
            } else {
                gap_detected = true;
                let matching: Vec<&CloudEvent> = lock.iter().filter(|event| filter.matches(event)).collect();
                match limit {
                    Some(max_count) => {
                        let skip = matching.len().saturating_sub(max_count);
                        matching.into_iter().skip(skip).cloned().collect()
                    }
                    None => matching.into_iter().cloned().collect(),
                }
            }
        } else {
            let matching: Vec<&CloudEvent> = lock.iter().filter(|event| filter.matches(event)).collect();
            match limit {
                Some(max_count) => {
                    let skip = matching.len().saturating_sub(max_count);
                    matching.into_iter().skip(skip).cloned().collect()
                }
                None => matching.into_iter().cloned().collect(),
            }
        };

        HistoryAscendingResult { events, gap_detected }
    }

    pub fn attach_source<S: EventSource>(&self, source: S) -> tokio::task::JoinHandle<()> {
        let bus = self.clone();
        tokio::spawn(async move {
            let mut backoff_secs = 1u64;
            let mut last_position: Option<Position> = None;
            loop {
                let position = match last_position.clone() {
                    Some(pos) => SubscribePosition::From(pos),
                    None => SubscribePosition::Live,
                };
                tracing::info!("Opening EventSource stream from position: {:?}", position);
                match source.open(position).await {
                    Ok(mut stream) => {
                        backoff_secs = 1;
                        while let Some(item) = stream.next().await {
                            match item {
                                Ok(source_event) => {
                                    if let Some(pos) = source_event.position {
                                        last_position = Some(pos);
                                    }
                                    bus.publish(source_event.event);
                                }
                                Err(err) => {
                                    tracing::warn!("EventSource stream item error: {:?}", err);
                                }
                            }
                        }
                        tracing::warn!("EventSource stream ended, reconnecting...");
                    }
                    Err(err) => {
                        tracing::error!("Failed to open EventSource stream: {:?}", err);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(30);
            }
        })
    }

    /// Converts this `EventBusHandle` into a boxed `Query<A>` publisher for a specific aggregate `A`.
    #[must_use]
    pub fn query<A>(&self) -> Box<dyn Query<A>>
    where
        A: Aggregate,
        A::Event: serde::Serialize + DomainEvent,
    {
        Box::new(self.clone())
    }
}

impl EventBus for EventBusHandle {
    fn subscribe(&self, filter: EventFilter) -> BusEventStream {
        let receiver = self.sender.subscribe();
        let stream = tokio_stream::wrappers::BroadcastStream::new(receiver).filter_map(move |result| {
            let filter = filter.clone();
            async move {
                match result {
                    Ok(event) => {
                        if filter.matches(&event) {
                            Some(Ok(event.as_ref().clone()))
                        } else {
                            None
                        }
                    }
                    Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                        Some(Err(EventBusError::Lagged(n)))
                    }
                }
            }
        });
        Box::pin(stream)
    }
}

#[async_trait]
impl<A> Query<A> for EventBusHandle
where
    A: Aggregate,
    A::Event: serde::Serialize + DomainEvent,
{
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<A>]) {
        for envelope in events {
            let payload = match serde_json::to_value(&envelope.payload) {
                Ok(val) => val,
                Err(err) => {
                    tracing::error!("Failed to serialize event payload for EventBus: {:?}", err);
                    continue;
                }
            };

            let occurred_at = envelope
                .metadata
                .get("timestamp")
                .and_then(|timestamp_str| chrono::DateTime::parse_from_rfc3339(timestamp_str).ok())
                .map(|datetime| datetime.with_timezone(&chrono::Utc))
                .or_else(|| Some(chrono::Utc::now()));

            let cloud_event = build_cloud_event(
                A::TYPE,
                aggregate_id,
                envelope.sequence,
                &envelope.payload.event_type(),
                payload,
                occurred_at,
            );

            self.publish(cloud_event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_cloud_event() {
        let event = build_cloud_event(
            "offer",
            "123",
            4,
            "OfferCreated",
            serde_json::json!({"id": "123"}),
            Some(Utc::now()),
        );
        assert_eq!(event.id, "offer:123:4");
        assert_eq!(event.source, "/services/offer");
        assert_eq!(event.event_type, "com.impierce.unicore.offer-created");
    }

    #[test]
    fn test_build_cloud_event_unwraps_tagged_variant() {
        let tagged_payload = serde_json::json!({
            "TemplateCreated": {
                "template_id": "tpl-1",
                "title": "Test Title"
            }
        });
        let event = build_cloud_event(
            "Template",
            "tpl-1",
            1,
            "TemplateCreated",
            tagged_payload,
            Some(Utc::now()),
        );

        assert_eq!(
            event.data,
            Some(serde_json::json!({
                "template_id": "tpl-1",
                "title": "Test Title"
            }))
        );
    }

    #[test]
    fn test_event_filter() {
        let event = build_cloud_event(
            "credential",
            "cred-1",
            1,
            "CredentialSigned",
            serde_json::json!({}),
            Some(Utc::now()),
        );

        let f1 = EventFilter {
            sources: vec!["/services/credential".to_string()],
            ..Default::default()
        };
        assert!(f1.matches(&event));

        let f2 = EventFilter {
            sources: vec!["/services/offer".to_string()],
            ..Default::default()
        };
        assert!(!f2.matches(&event));
    }

    #[tokio::test]
    async fn test_bus_fanout() {
        let handle = EventBusHandle::new(16);
        let mut stream = handle.subscribe(EventFilter::default());

        let event = build_cloud_event(
            "offer",
            "abc",
            1,
            "OfferCreated",
            serde_json::json!({"id": "abc"}),
            Some(Utc::now()),
        );

        handle.publish(event.clone());

        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received.subject, Some("abc".to_string()));
    }

    #[tokio::test]
    async fn test_publish_deduplication() {
        let handle = EventBusHandle::new(16);
        let mut stream = handle.subscribe(EventFilter::default());

        let event = build_cloud_event(
            "offer",
            "abc",
            1,
            "OfferCreated",
            serde_json::json!({"id": "abc"}),
            Some(Utc::now()),
        );

        handle.publish(event.clone());
        handle.publish(event.clone());

        let history = handle.history(&EventFilter::default(), 10);
        assert_eq!(history.len(), 1);

        let received = stream.next().await.unwrap().unwrap();
        assert_eq!(received.id, event.id);

        // Ensure second event was not published to subscribers
        tokio::select! {
            _ = stream.next() => panic!("Duplicate event should not be sent on subscriber stream"),
            () = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
    }

    #[tokio::test]
    async fn test_history_ascending_filter_with_limit() {
        let handle = EventBusHandle::new(16);

        // Publish an offer event, followed by several other events
        let target_event = build_cloud_event(
            "target",
            "1",
            1,
            "TargetCreated",
            serde_json::json!({}),
            Some(Utc::now()),
        );
        handle.publish(target_event.clone());

        for index in 2..=10 {
            let filler = build_cloud_event(
                "filler",
                &index.to_string(),
                1,
                "FillerCreated",
                serde_json::json!({}),
                Some(Utc::now()),
            );
            handle.publish(filler);
        }

        let filter = EventFilter {
            sources: vec!["/services/target".to_string()],
            ..Default::default()
        };

        // Query with limit Some(5)
        let result = handle.history_ascending(&filter, None, Some(5)).await;
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].id, target_event.id);

        // Query with unbounded limit None
        let result_unbounded = handle.history_ascending(&filter, None, None).await;
        assert_eq!(result_unbounded.events.len(), 1);
        assert_eq!(result_unbounded.events[0].id, target_event.id);
    }

    #[tokio::test]
    async fn test_attach_source_resumes_from_last_position() {
        struct MockEventSource {
            attempts: Arc<std::sync::atomic::AtomicUsize>,
        }

        #[async_trait]
        impl EventSource for MockEventSource {
            async fn open(&self, from: SubscribePosition) -> Result<EventSourceStream, EventBusError> {
                let attempt = self.attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if attempt == 0 {
                    assert_eq!(from, SubscribePosition::Live);
                    let event = build_cloud_event("test", "1", 1, "Created", serde_json::json!({}), None);
                    let source_event = SourceEvent::new(event, Some(Position(vec![42])));
                    let stream = futures::stream::iter(vec![Ok(source_event)]);
                    Ok(Box::pin(stream))
                } else {
                    assert_eq!(from, SubscribePosition::From(Position(vec![42])));
                    let event = build_cloud_event("test", "1", 2, "Updated", serde_json::json!({}), None);
                    let source_event = SourceEvent::new(event, Some(Position(vec![43])));
                    let stream = futures::stream::iter(vec![Ok(source_event)]);
                    Ok(Box::pin(stream))
                }
            }
        }

        let handle = EventBusHandle::new(16);
        let mut subscriber = handle.subscribe(EventFilter::default());

        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let join_handle = handle.attach_source(MockEventSource {
            attempts: attempts.clone(),
        });

        let first = tokio::time::timeout(std::time::Duration::from_millis(500), subscriber.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(first.id, "test:1:1");

        // After stream 1 ends, reconnect should pass From(Position(vec![42]))
        let second = tokio::time::timeout(std::time::Duration::from_millis(2000), subscriber.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(second.id, "test:1:2");

        join_handle.abort();
    }

    #[derive(Default, Debug, Serialize, Deserialize)]
    struct MockAggregate;

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    enum MockEvent {
        Created,
    }

    impl DomainEvent for MockEvent {
        fn event_type(&self) -> String {
            "Created".to_string()
        }
        fn event_version(&self) -> String {
            "1.0".to_string()
        }
    }

    impl Aggregate for MockAggregate {
        const TYPE: &'static str = "Mock";
        type Command = ();
        type Event = MockEvent;
        type Error = std::convert::Infallible;
        type Services = ();

        async fn handle(
            &mut self,
            _: Self::Command,
            _: &Self::Services,
            _: &cqrs_es::event_sink::EventSink<Self>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
        fn apply(&mut self, _: Self::Event) {}
    }

    #[tokio::test]
    async fn test_query_dispatch_preserves_metadata_timestamp() {
        let handle = EventBusHandle::new(16);
        let mut subscriber = handle.subscribe(EventFilter::default());

        let expected_time = DateTime::parse_from_rfc3339("2026-01-15T10:30:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("timestamp".to_string(), "2026-01-15T10:30:00Z".to_string());

        let envelope = cqrs_es::EventEnvelope {
            aggregate_id: "agg-1".to_string(),
            sequence: 1,
            payload: MockEvent::Created,
            metadata,
        };

        Query::<MockAggregate>::dispatch(&handle, "agg-1", &[envelope]).await;

        let received = subscriber.next().await.unwrap().unwrap();
        assert_eq!(received.time, Some(expected_time));
    }
}
