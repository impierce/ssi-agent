use crate::domain::event_publisher::{
    aggregate::EventPublisher,
    config::{EventPublisherConfig, EventPublisherFilter},
    error::EventPublisherError,
    event::EventPublisherEvent::EventPublisherCreated,
};
use chrono::Utc;
use shared_kernel::cqrs_es::{event_sink::EventSink, Aggregate};
use tracing::info;

impl EventPublisher {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn create_event_publisher(
        &mut self,
        id: String,
        name: String,
        description: Option<String>,
        enabled: bool,
        filter: EventPublisherFilter,
        config: EventPublisherConfig,
        service: &<EventPublisher as Aggregate>::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), EventPublisherError> {
        if !self.is_none() {
            return Err(EventPublisherError::AlreadyExists(id));
        }

        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(EventPublisherError::EmptyName);
        }

        let description = description.map(|d| d.trim().to_string()).filter(|d| !d.is_empty());

        let filter = service.known_event_types().validate_and_normalize_filter(filter)?;
        let config = config.normalized()?;

        info!(
            publisher_id = %id,
            publisher_type = ?config.publisher_type(),
            enabled,
            "Creating event publisher"
        );

        sink.write(
            EventPublisherCreated {
                id,
                name,
                description,
                enabled,
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
