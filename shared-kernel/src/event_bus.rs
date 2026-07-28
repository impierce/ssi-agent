use async_trait::async_trait;
use chrono::{DateTime, Utc};
use convert_case::{Case, Casing};
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, Query};
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;

/// A CNCF CloudEvent envelope (v1.0 spec).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CloudEvent {
    pub id: String,
    pub source: String,
    pub specversion: String,
    #[serde(rename = "type")]
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

    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }

    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }
}

/// Helper function to construct a reverse-DNS standard CNCF CloudEvent v1.0.
pub fn build_cloud_event(
    aggregate_type: &str,
    aggregate_id: &str,
    sequence: usize,
    event_type: &str,
    payload: serde_json::Value,
    occurred_at: Option<DateTime<Utc>>,
) -> CloudEvent {
    let cloud_type = format!("io.impierce.unicore.{}", event_type.to_case(Case::Kebab));
    let source = format!("/services/{}", aggregate_type.to_lowercase());
    let id = format!("{}:{}:{}", aggregate_type, aggregate_id, sequence);

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
    pub fn matches(&self, event: &CloudEvent) -> bool {
        if !self.event_types.is_empty()
            && !self.event_types.iter().any(|t| {
                t.eq_ignore_ascii_case(&event.event_type) || event.event_type.to_lowercase().contains(&t.to_lowercase())
            })
        {
            return false;
        }
        if !self.sources.is_empty()
            && !self.sources.iter().any(|s| {
                s.eq_ignore_ascii_case(&event.source) || event.source.to_lowercase().contains(&s.to_lowercase())
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position(pub Vec<u8>);

#[derive(Debug, Clone)]
pub enum SubscribePosition {
    Live,
    From(Position),
}

pub type BusEventStream = Pin<Box<dyn Stream<Item = Result<CloudEvent, EventBusError>> + Send>>;

/// Port for subscribing to the internal event bus.
pub trait EventBus: Send + Sync {
    fn subscribe(&self, filter: EventFilter) -> BusEventStream;
}

/// SPI for event source adapters (e.g., MongoDB Change Streams).
#[async_trait]
pub trait EventSource: Send + Sync + 'static {
    async fn open(&self, from: SubscribePosition) -> Result<BusEventStream, EventBusError>;
}

use std::collections::VecDeque;

/// In-process event bus handle backed by Tokio broadcast channel and recent event history ring-buffer.
#[derive(Clone)]
pub struct EventBusHandle {
    sender: tokio::sync::broadcast::Sender<Arc<CloudEvent>>,
    history: Arc<tokio::sync::RwLock<VecDeque<CloudEvent>>>,
    history_capacity: usize,
}

impl EventBusHandle {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(capacity);
        Self {
            sender,
            history: Arc::new(tokio::sync::RwLock::new(VecDeque::with_capacity(500))),
            history_capacity: 500,
        }
    }
}

impl Default for EventBusHandle {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl EventBusHandle {
    pub fn publish(&self, event: CloudEvent) {
        let _ = self.sender.send(Arc::new(event.clone()));

        let history = self.history.clone();
        let cap = self.history_capacity;
        tokio::spawn(async move {
            let mut lock = history.write().await;
            if lock.len() >= cap {
                lock.pop_front();
            }
            lock.push_back(event);
        });
    }

    pub async fn history(&self, filter: &EventFilter, limit: usize) -> Vec<CloudEvent> {
        let lock = self.history.read().await;
        lock.iter()
            .rev()
            .filter(|e| filter.matches(e))
            .take(limit)
            .cloned()
            .collect()
    }

    pub async fn history_ascending(
        &self,
        filter: &EventFilter,
        last_event_id: Option<&str>,
        limit: usize,
    ) -> Vec<CloudEvent> {
        let lock = self.history.read().await;

        if let Some(last_id) = last_event_id {
            if let Some(pos) = lock.iter().position(|e| e.id == last_id) {
                lock.iter()
                    .skip(pos + 1)
                    .filter(|e| filter.matches(e))
                    .cloned()
                    .collect()
            } else {
                let count = lock.len();
                let skip = count.saturating_sub(limit);
                lock.iter().skip(skip).filter(|e| filter.matches(e)).cloned().collect()
            }
        } else {
            let count = lock.len();
            let skip = count.saturating_sub(limit);
            lock.iter().skip(skip).filter(|e| filter.matches(e)).cloned().collect()
        }
    }

    pub fn attach_source<S: EventSource>(&self, source: S) -> tokio::task::JoinHandle<()> {
        let bus = self.clone();
        tokio::spawn(async move {
            let mut backoff_secs = 1u64;
            loop {
                tracing::info!("Opening EventSource stream...");
                match source.open(SubscribePosition::Live).await {
                    Ok(mut stream) => {
                        backoff_secs = 1;
                        while let Some(item) = stream.next().await {
                            match item {
                                Ok(event) => bus.publish(event),
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

            let cloud_event = build_cloud_event(
                A::TYPE,
                aggregate_id,
                envelope.sequence,
                &envelope.payload.event_type(),
                payload,
                Some(chrono::Utc::now()),
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
        assert_eq!(event.event_type, "io.impierce.unicore.offer-created");
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
}
