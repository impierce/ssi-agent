//! In-process and distributed event bus infrastructure using the CNCF `CloudEvents` v1.0 standard.
//!
//! This module provides:
//! - Standardized [`CloudEvent`] schema for event streaming and decoupling across services.
//! - [`EventFilter`] for filtering events by source, type, subject, and time ranges.
//! - [`EventBusHandle`] providing an in-process broadcast channel alongside a bounded ring-buffer for fast replay.
//! - [`EventSource`] and [`EventHistoryReader`] SPI traits allowing external persistent backends (such as `MongoDB` Change Streams) to feed live events and fulfill complete historical catch-up queries.
//! - Automatic bridging of `cqrs-es` domain events to `CloudEvent`s via [`Query<A>`] dispatch.

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
            && !self.event_types.iter().any(|pattern| {
                if pattern.eq_ignore_ascii_case(&event.event_type) {
                    return true;
                }
                let event_suffix = event
                    .event_type
                    .strip_prefix("com.impierce.unicore.")
                    .unwrap_or(&event.event_type);
                let pattern_kebab = pattern.to_case(Case::Kebab);
                event_suffix.eq_ignore_ascii_case(pattern) || event_suffix.eq_ignore_ascii_case(&pattern_kebab)
            })
        {
            return false;
        }
        if !self.sources.is_empty()
            && !self.sources.iter().any(|source_pattern| {
                let pattern_norm = source_pattern.trim_start_matches("/services/").trim_matches('/');
                let event_source_norm = event.source.trim_start_matches("/services/").trim_matches('/');
                source_pattern.eq_ignore_ascii_case(&event.source)
                    || pattern_norm.eq_ignore_ascii_case(event_source_norm)
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
            match event.time {
                Some(event_time) if event_time < since => return false,
                None => return false,
                _ => {}
            }
        }
        if let Some(until) = self.until {
            match event.time {
                Some(event_time) if event_time > until => return false,
                None => return false,
                _ => {}
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
    /// Returned by an [`EventSource`] given a [`Position`] it cannot interpret.
    #[error("Position-based subscription is unsupported")]
    UnsupportedPosition,
    /// Reserved for [`EventSource`] and [`EventHistoryReader`] implementations whose backing
    /// stream can terminate; the in-process bus never closes.
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

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

/// How long a published event's identifier is remembered for duplicate suppression.
///
/// A duration rather than an event count because the gap it has to span is one: the change
/// stream's delivery lag behind in-process dispatch, which stretches to the reconnect backoff.
const DEDUP_WINDOW_TTL: Duration = Duration::from_secs(600);

/// Caps memory when [`DEDUP_WINDOW_TTL`] would retain more than this under sustained throughput.
const DEDUP_WINDOW_MAX_ENTRIES: usize = 50_000;

/// Recently published event identifiers, evicted by age and by count.
#[derive(Debug)]
struct DedupWindow {
    ids: HashSet<String>,
    order: VecDeque<(String, Instant)>,
    ttl: Duration,
    max_entries: usize,
}

impl DedupWindow {
    fn new() -> Self {
        Self::with_limits(DEDUP_WINDOW_TTL, DEDUP_WINDOW_MAX_ENTRIES)
    }

    fn with_limits(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ids: HashSet::new(),
            order: VecDeque::new(),
            ttl,
            max_entries,
        }
    }

    /// Records `id`, returning `false` if it was already inside the window — i.e. a duplicate.
    fn insert(&mut self, id: &str) -> bool {
        let now = Instant::now();

        while let Some((_, recorded_at)) = self.order.front() {
            let expired = now.duration_since(*recorded_at) >= self.ttl;
            if !expired && self.order.len() <= self.max_entries {
                break;
            }
            if let Some((oldest_id, _)) = self.order.pop_front() {
                self.ids.remove(&oldest_id);
            }
        }

        if !self.ids.insert(id.to_string()) {
            return false;
        }
        self.order.push_back((id.to_string(), now));
        true
    }
}

/// In-process event bus handle backed by Tokio broadcast channel and recent event history ring-buffer.
#[derive(Clone)]
pub struct EventBusHandle {
    sender: tokio::sync::broadcast::Sender<Arc<CloudEvent>>,
    history: Arc<std::sync::RwLock<VecDeque<CloudEvent>>>,
    history_capacity: usize,
    history_reader: Arc<std::sync::RwLock<Option<Arc<dyn EventHistoryReader>>>>,
    dedup: Arc<std::sync::RwLock<DedupWindow>>,
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
            dedup: Arc::new(std::sync::RwLock::new(DedupWindow::new())),
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
/// Contains the list of matching [`CloudEvent`]s plus two independent gap signals:
/// `gap_detected` (the requested `last_event_id` was absent from memory/storage) and
/// `truncated` (catch-up stopped before all matching events were read).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HistoryAscendingResult {
    pub events: Vec<CloudEvent>,
    /// Set to `true` if `last_event_id` was specified but could not be found in retained history.
    pub gap_detected: bool,
    /// Set when the reader stopped on a limit rather than on exhausting matching events.
    ///
    /// Returning fewer than the requested number of *latest* events (no `last_event_id`) is not
    /// truncation — that is the caller's own limit.
    pub truncated: bool,
    /// Where a truncated resume should continue from, as a `last_event_id` value.
    ///
    /// Tracks the last event *examined* rather than the last returned, so that a scan which
    /// matched nothing still advances instead of repeating itself on the next attempt.
    pub resume_after: Option<String>,
}

impl EventBusHandle {
    /// Publishes a [`CloudEvent`] to all active subscribers and synchronously appends it to the in-memory ring-buffer.
    ///
    /// In `MongoDB` deployments each event reaches the bus twice — from in-process
    /// [`Query::dispatch`] and from the change stream — so the second copy is dropped if it falls
    /// inside the [`DedupWindow`].
    pub fn publish(&self, event: CloudEvent) {
        {
            let mut dedup = match self.dedup.write() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if !dedup.insert(&event.id) {
                return;
            }
        }

        {
            let mut lock = match self.history.write() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if lock.len() >= self.history_capacity {
                lock.pop_front();
            }
            lock.push_back(event.clone());
        }

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
    ///
    /// # Errors
    ///
    /// Returns an [`EventBusError`] if the underlying [`EventHistoryReader`] fails.
    pub async fn history_ascending(
        &self,
        filter: &EventFilter,
        last_event_id: Option<&str>,
        limit: Option<usize>,
    ) -> Result<HistoryAscendingResult, EventBusError> {
        let reader = {
            let lock = match self.history_reader.read() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            lock.clone()
        };

        // 1. Delegate to persistent storage reader (e.g. MongoDB) if configured.
        // Storage errors are propagated immediately (fail-closed) to ensure clients are not
        // silently served an incomplete catch-up stream that masks database issues.
        if let Some(reader) = reader {
            return reader.history_ascending(filter, last_event_id, limit).await;
        }

        // 2. Fall back to the bounded in-memory ring buffer (e.g. in test or standalone environments).
        let lock = match self.history.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let mut gap_detected = false;
        let mut truncated = false;

        let events: Vec<CloudEvent> = if let Some(last_id) = last_event_id {
            // Find the position of the last acknowledged event in the in-memory ring buffer.
            if let Some(pos) = lock.iter().position(|event| event.id == last_id) {
                // Resume strictly after last_id in chronological order.
                let iter = lock.iter().skip(pos + 1).filter(|event| filter.matches(event));
                match limit {
                    // A live-only request asked for no catch-up, so nothing is truncated.
                    Some(0) => Vec::new(),
                    Some(max_count) => {
                        // One extra distinguishes "exactly max_count matched" from "stopped early".
                        let mut collected: Vec<CloudEvent> = iter.take(max_count.saturating_add(1)).cloned().collect();
                        truncated = collected.len() > max_count;
                        collected.truncate(max_count);
                        collected
                    }
                    None => iter.cloned().collect(),
                }
            } else {
                // The requested event was either never present or evicted from the ring-buffer.
                // Mark gap_detected = true so callers can emit a lagged/warning notice.
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
            // No last_event_id: return latest matching events in ascending order.
            let matching: Vec<&CloudEvent> = lock.iter().filter(|event| filter.matches(event)).collect();
            match limit {
                Some(max_count) => {
                    let skip = matching.len().saturating_sub(max_count);
                    matching.into_iter().skip(skip).cloned().collect()
                }
                None => matching.into_iter().cloned().collect(),
            }
        };

        let resume_after = events.last().map(|event| event.id.clone());

        Ok(HistoryAscendingResult {
            events,
            gap_detected,
            truncated,
            resume_after,
        })
    }

    /// Attaches an external [`EventSource`] (such as a database Change Stream) to feed the bus in a background task.
    ///
    /// Supervised loop with exponential backoff that tracks [`Position`] resume tokens to ensure zero dropped events
    /// across stream interruptions and cluster failovers.
    pub fn attach_source<S: EventSource>(&self, source: S) -> tokio::task::JoinHandle<()> {
        let bus = self.clone();
        tokio::spawn(async move {
            let mut backoff_millis = 100u64;
            let mut last_position: Option<Position> = None;
            let mut failed_resume_attempts: usize = 0;
            let mut consecutive_open_failures: usize = 0;
            loop {
                // Initial connect starts from Live; subsequent reconnects resume from the last known stream position.
                let position = match last_position.clone() {
                    Some(pos) => SubscribePosition::From(pos),
                    None => SubscribePosition::Live,
                };
                tracing::info!("Opening EventSource stream from position: {:?}", position);
                match source.open(position).await {
                    Ok(mut stream) => {
                        backoff_millis = 100;
                        failed_resume_attempts = 0;
                        consecutive_open_failures = 0;
                        while let Some(item) = stream.next().await {
                            match item {
                                Ok(source_event) => {
                                    // Update the resume position checkpoint.
                                    if let Some(pos) = source_event.position {
                                        last_position = Some(pos);
                                    }
                                    // Publish to local bus. Duplicates are automatically discarded by the history buffer.
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
                        // Change streams need a replica set, so on a standalone mongod this fails
                        // permanently. Report the first failure loudly and the repeats quietly.
                        if consecutive_open_failures == 0 {
                            tracing::error!("Failed to open EventSource stream: {:?}", err);
                        } else {
                            tracing::debug!(
                                "Failed to open EventSource stream ({} consecutive failures): {:?}",
                                consecutive_open_failures + 1,
                                err
                            );
                        }
                        consecutive_open_failures += 1;

                        if last_position.is_some() {
                            failed_resume_attempts += 1;
                            if failed_resume_attempts >= 3 {
                                tracing::warn!(
                                    "Failed to resume EventSource from position after 3 consecutive attempts; falling back to SubscribePosition::Live: {:?}",
                                    err
                                );
                                last_position = None;
                                failed_resume_attempts = 0;
                                backoff_millis = 100;
                            }
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(backoff_millis)).await;
                backoff_millis = (backoff_millis * 2).min(10_000);
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
                    Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(dropped_count)) => {
                        Some(Err(EventBusError::Lagged(dropped_count)))
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
    /// Bridges cqrs-es domain events to `CloudEvents` on the bus when aggregates persist changes.
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<A>]) {
        for envelope in events {
            let payload = match serde_json::to_value(&envelope.payload) {
                Ok(val) => val,
                Err(err) => {
                    tracing::error!("Failed to serialize event payload for EventBus: {:?}", err);
                    continue;
                }
            };

            // Extract the original timestamp recorded in the envelope's metadata (set during command handling)
            // so CloudEvent.time accurately reflects when the domain event occurred rather than when it was dispatched.
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

        // Exact URI match
        let f1 = EventFilter {
            sources: vec!["/services/credential".to_string()],
            ..Default::default()
        };
        assert!(f1.matches(&event));

        // Bare aggregate name without /services/ prefix
        let f2 = EventFilter {
            sources: vec!["credential".to_string()],
            ..Default::default()
        };
        assert!(f2.matches(&event));

        // Non-matching source
        let f3 = EventFilter {
            sources: vec!["/services/offer".to_string()],
            ..Default::default()
        };
        assert!(!f3.matches(&event));

        // Substring non-match: "cred" should not match "/services/credential"
        let f4 = EventFilter {
            sources: vec!["cred".to_string()],
            ..Default::default()
        };
        assert!(!f4.matches(&event));

        // Type filter matching domain event name and kebab-case suffix
        let f5 = EventFilter {
            event_types: vec!["CredentialSigned".to_string()],
            ..Default::default()
        };
        assert!(f5.matches(&event));

        let f6 = EventFilter {
            event_types: vec!["credential-signed".to_string()],
            ..Default::default()
        };
        assert!(f6.matches(&event));

        // Partial non-suffix match should not match
        let f7 = EventFilter {
            event_types: vec!["Signed".to_string()],
            ..Default::default()
        };
        assert!(!f7.matches(&event));

        // Timeless event does not match when since is set
        let mut timeless_event = CloudEvent::new("com.impierce.unicore.test", "/services/test");
        timeless_event.time = None;
        let f8 = EventFilter {
            since: Some(Utc::now()),
            ..Default::default()
        };
        assert!(!f8.matches(&timeless_event));
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
        let result = handle.history_ascending(&filter, None, Some(5)).await.unwrap();
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].id, target_event.id);

        // Query with unbounded limit None
        let result_unbounded = handle.history_ascending(&filter, None, None).await.unwrap();
        assert_eq!(result_unbounded.events.len(), 1);
        assert_eq!(result_unbounded.events[0].id, target_event.id);
    }

    #[tokio::test]
    async fn test_history_ascending_propagates_reader_error() {
        struct FailingReader;

        #[async_trait]
        impl EventHistoryReader for FailingReader {
            async fn history_ascending(
                &self,
                _filter: &EventFilter,
                _last_event_id: Option<&str>,
                _limit: Option<usize>,
            ) -> Result<HistoryAscendingResult, EventBusError> {
                Err(EventBusError::Source("connection timeout".to_string()))
            }
        }

        let handle = EventBusHandle::new(16);
        handle.set_history_reader(Arc::new(FailingReader));

        let filter = EventFilter::default();
        let result = handle.history_ascending(&filter, None, None).await;
        assert!(matches!(result, Err(EventBusError::Source(msg)) if msg == "connection timeout"));
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
        let second = tokio::time::timeout(std::time::Duration::from_secs(2), subscriber.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(second.id, "test:1:2");

        join_handle.abort();
    }

    #[tokio::test]
    async fn test_attach_source_falls_back_to_live_on_repeated_resume_failures() {
        struct StalePositionSource {
            calls: Arc<std::sync::atomic::AtomicUsize>,
            recovered: Arc<std::sync::atomic::AtomicBool>,
        }

        #[async_trait]
        impl EventSource for StalePositionSource {
            async fn open(&self, from: SubscribePosition) -> Result<EventSourceStream, EventBusError> {
                let call_idx = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                match from {
                    SubscribePosition::Live if call_idx == 0 => {
                        let event = build_cloud_event("test", "1", 1, "Init", serde_json::json!({}), None);
                        let source_event = SourceEvent::new(event, Some(Position(vec![99])));
                        Ok(Box::pin(futures::stream::iter(vec![Ok(source_event)])))
                    }
                    SubscribePosition::From(_) => Err(EventBusError::Source("ChangeStreamHistoryLost".to_string())),
                    SubscribePosition::Live => {
                        self.recovered.store(true, std::sync::atomic::Ordering::SeqCst);
                        let event = build_cloud_event("test", "1", 2, "Recovered", serde_json::json!({}), None);
                        Ok(Box::pin(futures::stream::iter(vec![Ok(SourceEvent::new(event, None))])))
                    }
                }
            }
        }

        let handle = EventBusHandle::new(16);
        let mut subscriber = handle.subscribe(EventFilter::default());

        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let recovered = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let join_handle = handle.attach_source(StalePositionSource {
            calls: calls.clone(),
            recovered: recovered.clone(),
        });

        let first = tokio::time::timeout(std::time::Duration::from_millis(500), subscriber.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(first.id, "test:1:1");

        // After 3 failed attempts from position, attach_source falls back to Live and yields second event
        let second = tokio::time::timeout(std::time::Duration::from_secs(12), subscriber.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(second.id, "test:1:2");
        assert!(recovered.load(std::sync::atomic::Ordering::SeqCst));

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
            (): Self::Command,
            (): &Self::Services,
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

    #[tokio::test]
    async fn history_ascending_reports_truncation_when_resume_hits_the_limit() {
        let handle = EventBusHandle::new(16);
        for sequence in 1..=5 {
            handle.publish(build_cloud_event(
                "credential",
                "cred-1",
                sequence,
                "CredentialSigned",
                serde_json::json!({}),
                None,
            ));
        }

        let filter = EventFilter::default();

        // Three events follow `cred-1:2`, so a limit of two stops short of exhausting them.
        let result = handle
            .history_ascending(&filter, Some("credential:cred-1:2"), Some(2))
            .await
            .unwrap();
        assert_eq!(result.events.len(), 2);
        assert!(result.truncated);
        assert!(!result.gap_detected);

        // A limit that exactly covers the remaining events is not truncation.
        let result = handle
            .history_ascending(&filter, Some("credential:cred-1:2"), Some(3))
            .await
            .unwrap();
        assert_eq!(result.events.len(), 3);
        assert!(!result.truncated);

        // Neither is a limit wider than what remains.
        let result = handle
            .history_ascending(&filter, Some("credential:cred-1:2"), Some(100))
            .await
            .unwrap();
        assert_eq!(result.events.len(), 3);
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn history_ascending_does_not_report_truncation_for_the_latest_events() {
        let handle = EventBusHandle::new(16);
        for sequence in 1..=5 {
            handle.publish(build_cloud_event(
                "credential",
                "cred-1",
                sequence,
                "CredentialSigned",
                serde_json::json!({}),
                None,
            ));
        }

        // Without `last_event_id` the caller asked for the latest N; older events being left out
        // is the requested semantics, not a gap.
        let result = handle
            .history_ascending(&EventFilter::default(), None, Some(2))
            .await
            .unwrap();
        assert_eq!(result.events.len(), 2);
        assert!(!result.truncated);

        // An unresolvable `last_event_id` falls back to the same "latest N" shape, and is already
        // signalled by `gap_detected`.
        let result = handle
            .history_ascending(&EventFilter::default(), Some("credential:cred-1:999"), Some(2))
            .await
            .unwrap();
        assert!(result.gap_detected);
        assert!(!result.truncated);
    }

    #[test]
    fn dedup_window_evicts_by_count() {
        let mut window = DedupWindow::with_limits(Duration::from_secs(600), 3);

        assert!(window.insert("a"));
        assert!(window.insert("b"));
        assert!(window.insert("c"));
        // Still inside the window.
        assert!(!window.insert("a"));

        // "d" pushes the window past its cap, evicting the oldest entry.
        assert!(window.insert("d"));
        assert!(window.insert("a"), "oldest entry should have been evicted");
    }

    #[test]
    fn dedup_window_evicts_by_age() {
        let mut window = DedupWindow::with_limits(Duration::ZERO, DEDUP_WINDOW_MAX_ENTRIES);

        assert!(window.insert("a"));
        // A zero TTL expires every entry before the next insert is checked.
        assert!(window.insert("a"));
    }

    #[tokio::test]
    async fn publish_suppresses_duplicates_beyond_the_history_ring_buffer() {
        let handle = EventBusHandle::new(4096);

        let replayed = build_cloud_event(
            "credential",
            "cred-0",
            1,
            "CredentialSigned",
            serde_json::json!({}),
            None,
        );
        handle.publish(replayed.clone());

        // Fill well past the 500-event history ring-buffer.
        for sequence in 1..=600 {
            handle.publish(build_cloud_event(
                "credential",
                "cred-filler",
                sequence,
                "CredentialSigned",
                serde_json::json!({}),
                None,
            ));
        }

        let mut subscriber = handle.subscribe(EventFilter::default());

        // Replayed by the change stream: long gone from the ring-buffer, still inside the window.
        handle.publish(replayed.clone());

        let follow_up = build_cloud_event(
            "credential",
            "cred-1",
            1,
            "CredentialSigned",
            serde_json::json!({}),
            None,
        );
        handle.publish(follow_up.clone());

        let received = subscriber.next().await.unwrap().unwrap();
        assert_eq!(
            received.id, follow_up.id,
            "replayed duplicate should not have been broadcast"
        );
    }

    #[tokio::test]
    async fn history_ascending_treats_a_zero_limit_as_live_only() {
        let handle = EventBusHandle::new(16);
        for sequence in 1..=5 {
            handle.publish(build_cloud_event(
                "credential",
                "cred-1",
                sequence,
                "CredentialSigned",
                serde_json::json!({}),
                None,
            ));
        }

        // `?limit=0` asks for no catch-up at all, so events existing after the resume point is not
        // truncation. The MongoDB reader returns early for the same reason.
        let result = handle
            .history_ascending(&EventFilter::default(), Some("credential:cred-1:2"), Some(0))
            .await
            .unwrap();
        assert!(result.events.is_empty());
        assert!(!result.truncated, "a live-only request must not report truncation");

        // Without a resume point the other branches already behaved this way.
        let result = handle
            .history_ascending(&EventFilter::default(), None, Some(0))
            .await
            .unwrap();
        assert!(result.events.is_empty());
        assert!(!result.truncated);
    }
}
