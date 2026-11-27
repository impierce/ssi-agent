use crate::domain::event_publisher::aggregate::EventPublisher;

impl EventPublisher {
    pub fn event_publisher_deleted(&mut self) {
        *self = EventPublisher::None;
    }
}
