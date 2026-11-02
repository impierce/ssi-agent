use crate::domain::event_publisher::{
    aggregate::EventPublisher, config::EventPublisherConfig, error::EventPublisherError,
    event::EventPublisherEvent::EventPublisherUpdated,
};
use chrono::Utc;
use shared_kernel::cqrs_es::{event_sink::EventSink, Aggregate};
use tracing::info;

impl EventPublisher {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn update_event_publisher(
        &mut self,
        id: String,
        name: Option<String>,
        description: Option<Option<String>>,
        events: Option<Vec<String>>,
        config: Option<EventPublisherConfig>,
        service: &<EventPublisher as Aggregate>::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), EventPublisherError> {
        let current = match self {
            EventPublisher::Configured(inner) => inner.clone(),
            EventPublisher::None => return Err(EventPublisherError::NotFound(id)),
        };

        let updated_name = match name {
            Some(name) => {
                let trimmed = name.trim().to_string();
                if trimmed.is_empty() {
                    return Err(EventPublisherError::EmptyName);
                }
                trimmed
            }
            None => current.name.clone(),
        };

        let updated_description = match description {
            Some(Some(desc)) => {
                let trimmed = desc.trim().to_string();
                (!trimmed.is_empty()).then_some(trimmed)
            }
            Some(None) => None,
            None => current.description.clone(),
        };

        let updated_events = match events {
            Some(events) => service.known_event_types().validate_and_normalize(events)?,
            None => current.events.clone(),
        };

        let updated_config = match config {
            Some(config) => config.with_preserved_secrets(&current.config).normalized()?,
            None => current.config.clone(),
        };

        if updated_name == current.name
            && updated_description == current.description
            && updated_events == current.events
            && updated_config == current.config
        {
            info!(publisher_id = %current.id, "Event publisher update submitted unchanged; skipping event write");
            return Ok(());
        }

        info!(
            publisher_id = %current.id,
            publisher_type = ?updated_config.publisher_type(),
            "Updating event publisher"
        );

        sink.write(
            EventPublisherUpdated {
                id: current.id,
                name: updated_name,
                description: updated_description,
                enabled: current.enabled,
                events: updated_events,
                config: updated_config,
                occurred_at: Utc::now(),
            },
            self,
        )
        .await;

        Ok(())
    }
}
