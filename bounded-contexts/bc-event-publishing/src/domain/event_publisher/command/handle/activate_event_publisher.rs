use crate::domain::event_publisher::{
    aggregate::EventPublisher, error::EventPublisherError, event::EventPublisherEvent::EventPublisherActivated,
};
use chrono::Utc;
use shared_kernel::cqrs_es::{event_sink::EventSink, Aggregate};
use tracing::info;

impl EventPublisher {
    /// Idempotent activation of an event publisher.
    pub(crate) async fn activate_event_publisher(
        &mut self,
        id: String,
        sink: &EventSink<Self>,
    ) -> Result<(), <EventPublisher as Aggregate>::Error> {
        let (publisher_id, already_enabled, filter, config) = match self {
            EventPublisher::Configured(inner) => (
                inner.id.clone(),
                inner.enabled,
                inner.filter.clone(),
                inner.config.clone(),
            ),
            EventPublisher::None => return Err(EventPublisherError::NotFound(id)),
        };

        if already_enabled {
            info!(publisher_id = %publisher_id, "Event publisher is already active; skipping event write");
            return Ok(());
        }

        info!(publisher_id = %publisher_id, "Activating event publisher");
        sink.write(
            EventPublisherActivated {
                id: publisher_id,
                filter,
                config,
                occurred_at: Utc::now(),
            },
            self,
        )
        .await;

        Ok(())
    }
}
