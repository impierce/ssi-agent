use crate::domain::event_publisher::aggregate::EventPublisher;
use tracing::debug;

impl EventPublisher {
    pub(crate) fn event_publisher_deleted(&mut self) {
        debug!("Applying EventPublisherDeleted event");
        *self = EventPublisher::None;
    }
}
