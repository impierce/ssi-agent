use crate::domain::event_publisher::{
    aggregate::{EventPublisher, EventPublisherInner},
    config::{EventPublisherConfig, EventPublisherFilter},
};
use chrono::{DateTime, Utc};
use tracing::debug;

impl EventPublisher {
    #[allow(clippy::too_many_arguments)]
    pub fn event_publisher_created(
        &mut self,
        id: String,
        name: String,
        description: Option<String>,
        enabled: bool,
        filter: EventPublisherFilter,
        config: EventPublisherConfig,
        occurred_at: DateTime<Utc>,
    ) {
        debug!(id = %id, "Applying EventPublisherCreated event");
        *self = EventPublisher::Configured(Box::new(EventPublisherInner {
            id,
            name,
            description,
            enabled,
            filter,
            config,
            created_at: occurred_at,
            last_modified_at: occurred_at,
        }));
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::domain::event_publisher::{
        aggregate::tests::{http_config, publisher_id},
        event::EventPublisherEvent,
    };
    use rstest::fixture;

    #[fixture]
    pub fn created_http_event(publisher_id: String, http_config: EventPublisherConfig) -> EventPublisherEvent {
        EventPublisherEvent::EventPublisherCreated {
            id: publisher_id,
            name: "CRM Webhook".to_string(),
            description: Some("Forwards issuance events".to_string()),
            enabled: true,
            filter: EventPublisherFilter {
                types: vec!["com.impierce.unicore.issuance.credential.issued".to_string()],
            },
            config: http_config,
            occurred_at: Utc::now(),
        }
    }
}
