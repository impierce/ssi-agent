use crate::domain::event_publisher::{
    aggregate::EventPublisher, error::EventPublisherError, event::EventPublisherEvent::EventPublisherDeleted,
};
use chrono::Utc;
use shared_kernel::cqrs_es::{event_sink::EventSink, Aggregate};
use tracing::info;

impl EventPublisher {
    /// Removes an event publisher, stopping any active background delivery task.
    pub(crate) async fn delete_event_publisher(
        &mut self,
        id: String,
        sink: &EventSink<Self>,
    ) -> Result<(), <EventPublisher as Aggregate>::Error> {
        let publisher_id = match self {
            EventPublisher::Configured(inner) => inner.id.clone(),
            EventPublisher::None => return Err(EventPublisherError::NotFound(id)),
        };

        info!(publisher_id = %publisher_id, "Deleting event publisher");
        sink.write(
            EventPublisherDeleted {
                id: publisher_id,
                occurred_at: Utc::now(),
            },
            self,
        )
        .await;

        Ok(())
    }
}
