use crate::domain::event_publisher::{
    aggregate::{EventPublisher, EventPublisherInner},
    config::EventPublisherConfig,
};
use chrono::{DateTime, Utc};

impl EventPublisher {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn event_publisher_created(
        &mut self,
        id: String,
        name: String,
        description: Option<String>,
        enabled: bool,
        events: Vec<String>,
        config: EventPublisherConfig,
        occurred_at: DateTime<Utc>,
    ) {
        *self = EventPublisher::Configured(Box::new(EventPublisherInner {
            id,
            name,
            description,
            enabled,
            events,
            config,
            created_at: occurred_at,
            last_modified_at: occurred_at,
        }));
    }

    pub(crate) fn event_publisher_updated(
        &mut self,
        name: String,
        description: Option<String>,
        enabled: bool,
        events: Vec<String>,
        config: EventPublisherConfig,
        occurred_at: DateTime<Utc>,
    ) {
        if let EventPublisher::Configured(inner) = self {
            inner.name = name;
            inner.description = description;
            inner.enabled = enabled;
            inner.events = events;
            inner.config = config;
            inner.last_modified_at = occurred_at;
        }
    }

    pub(crate) fn event_publisher_activated(&mut self, occurred_at: DateTime<Utc>) {
        if let EventPublisher::Configured(inner) = self {
            inner.enabled = true;
            inner.last_modified_at = occurred_at;
        }
    }

    pub(crate) fn event_publisher_deactivated(&mut self, occurred_at: DateTime<Utc>) {
        if let EventPublisher::Configured(inner) = self {
            inner.enabled = false;
            inner.last_modified_at = occurred_at;
        }
    }

    pub(crate) fn event_publisher_deleted(&mut self) {
        *self = EventPublisher::None;
    }
}
