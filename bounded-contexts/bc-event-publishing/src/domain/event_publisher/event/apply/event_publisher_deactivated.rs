use crate::domain::event_publisher::aggregate::EventPublisher;
use chrono::{DateTime, Utc};

impl EventPublisher {
    pub fn event_publisher_deactivated(&mut self, occurred_at: DateTime<Utc>) {
        if let EventPublisher::Configured(inner) = self {
            inner.enabled = false;
            inner.last_modified_at = occurred_at;
        }
    }
}
