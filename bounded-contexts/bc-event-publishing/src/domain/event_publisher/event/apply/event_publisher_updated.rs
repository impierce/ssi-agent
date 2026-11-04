use crate::domain::event_publisher::{
    aggregate::EventPublisher,
    config::{EventPublisherConfig, EventPublisherFilter},
};
use chrono::{DateTime, Utc};
use tracing::debug;

impl EventPublisher {
    pub(crate) fn event_publisher_updated(
        &mut self,
        name: String,
        description: Option<String>,
        enabled: bool,
        filter: EventPublisherFilter,
        config: EventPublisherConfig,
        occurred_at: DateTime<Utc>,
    ) {
        if let EventPublisher::Configured(inner) = self {
            debug!(id = %inner.id, "Applying EventPublisherUpdated event");
            inner.name = name;
            inner.description = description;
            inner.enabled = enabled;
            inner.filter = filter;
            inner.config = config;
            inner.last_modified_at = occurred_at;
        }
    }
}
