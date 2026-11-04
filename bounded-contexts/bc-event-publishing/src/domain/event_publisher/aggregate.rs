use crate::domain::event_publisher::{
    command::EventPublisherCommand::{
        self, ActivateEventPublisher, CreateEventPublisher, DeactivateEventPublisher, DeleteEventPublisher,
        UpdateEventPublisher,
    },
    config::{EventPublisherConfig, EventPublisherFilter},
    error::EventPublisherError,
    event::EventPublisherEvent::{
        self, EventPublisherActivated, EventPublisherCreated, EventPublisherDeactivated, EventPublisherDeleted,
        EventPublisherUpdated,
    },
    services::EventPublisherDomainServices,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use shared_kernel::cqrs_es::{event_sink::EventSink, Aggregate};
use tracing::{debug, info};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct EventPublisherInner {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub filter: EventPublisherFilter,
    pub config: EventPublisherConfig,
    pub created_at: DateTime<Utc>,
    pub last_modified_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
pub enum EventPublisher {
    Configured(Box<EventPublisherInner>),
    #[default]
    None,
}

impl Aggregate for EventPublisher {
    const TYPE: &'static str = "event_publisher";
    type Command = EventPublisherCommand;
    type Event = EventPublisherEvent;
    type Error = EventPublisherError;
    type Services = EventPublisherDomainServices;

    async fn handle(
        &mut self,
        command: Self::Command,
        service: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        info!("Handling event publisher command: {:?}", command);
        debug!("Current event publisher state: {:?}", self);

        match command {
            CreateEventPublisher {
                id,
                name,
                description,
                enabled,
                filter,
                config,
            } => {
                self.create_event_publisher(id, name, description, enabled, filter, config, service, sink)
                    .await?;
            }
            UpdateEventPublisher {
                id,
                name,
                description,
                filter,
                config,
            } => {
                self.update_event_publisher(id, name, description, filter, config, service, sink)
                    .await?;
            }
            ActivateEventPublisher { id } => {
                self.activate_event_publisher(id, sink).await?;
            }
            DeactivateEventPublisher { id } => {
                self.deactivate_event_publisher(id, sink).await?;
            }
            DeleteEventPublisher { id } => {
                self.delete_event_publisher(id, sink).await?;
            }
        }

        info!("Successfully handled event publisher command");
        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            EventPublisherCreated {
                id,
                name,
                description,
                enabled,
                filter,
                config,
                occurred_at,
            } => {
                self.event_publisher_created(id, name, description, enabled, filter, config, occurred_at);
            }
            EventPublisherUpdated {
                id: _,
                name,
                description,
                enabled,
                filter,
                config,
                occurred_at,
            } => {
                self.event_publisher_updated(name, description, enabled, filter, config, occurred_at);
            }
            EventPublisherActivated { occurred_at, .. } => {
                self.event_publisher_activated(occurred_at);
            }
            EventPublisherDeactivated { occurred_at, .. } => {
                self.event_publisher_deactivated(occurred_at);
            }
            EventPublisherDeleted { .. } => {
                self.event_publisher_deleted();
            }
        }
    }
}

impl EventPublisher {
    #[must_use]
    pub fn is_configured(&self) -> bool {
        matches!(self, EventPublisher::Configured(_))
    }

    #[must_use]
    pub fn is_none(&self) -> bool {
        matches!(self, EventPublisher::None)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::domain::event_publisher::config::{HttpPublisherConfig, NatsPublisherConfig};
    use rstest::fixture;
    use std::collections::BTreeMap;

    #[fixture]
    pub fn publisher_id() -> String {
        "f2202574-9a5b-5ecd-a1a3-df5ca8e87354".to_string()
    }

    #[fixture]
    pub fn http_config() -> EventPublisherConfig {
        let mut headers = BTreeMap::new();
        headers.insert("authorization".to_string(), "Bearer super-secret".to_string());
        EventPublisherConfig::Http(HttpPublisherConfig {
            target_url: "https://example.test/webhooks/unicore".to_string(),
            headers: Some(headers),
        })
    }

    #[fixture]
    pub fn nats_config() -> EventPublisherConfig {
        EventPublisherConfig::Nats(NatsPublisherConfig {
            nats_url: "nats://127.0.0.1:4222".to_string(),
            subject: "unicore.events".to_string(),
        })
    }

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
