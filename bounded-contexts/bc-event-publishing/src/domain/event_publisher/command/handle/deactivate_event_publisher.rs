use crate::domain::event_publisher::{
    aggregate::EventPublisher, error::EventPublisherError, event::EventPublisherEvent::EventPublisherDeactivated,
};
use chrono::Utc;
use shared_kernel::cqrs_es::{event_sink::EventSink, Aggregate};
use tracing::info;

impl EventPublisher {
    /// Idempotent deactivation of an event publisher.
    pub(crate) async fn deactivate_event_publisher(
        &mut self,
        id: String,
        sink: &EventSink<Self>,
    ) -> Result<(), <EventPublisher as Aggregate>::Error> {
        let (publisher_id, already_disabled) = match self {
            EventPublisher::Configured(inner) => (inner.id.clone(), !inner.enabled),
            EventPublisher::None => return Err(EventPublisherError::NotFound(id)),
        };

        if already_disabled {
            info!(publisher_id = %publisher_id, "Event publisher is already inactive; skipping event write");
            return Ok(());
        }

        info!(publisher_id = %publisher_id, "Deactivating event publisher");
        sink.write(
            EventPublisherDeactivated {
                id: publisher_id,
                occurred_at: Utc::now(),
            },
            self,
        )
        .await;

        Ok(())
    }
}
