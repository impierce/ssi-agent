use crate::domain::event_publisher::aggregate::EventPublisher;
use chrono::{DateTime, Utc};
use tracing::debug;

impl EventPublisher {
    pub(crate) fn event_publisher_activated(&mut self, occurred_at: DateTime<Utc>) {
        if let EventPublisher::Configured(inner) = self {
            debug!(id = %inner.id, "Applying EventPublisherActivated event");
            inner.enabled = true;
            inner.last_modified_at = occurred_at;
        }
    }
}
