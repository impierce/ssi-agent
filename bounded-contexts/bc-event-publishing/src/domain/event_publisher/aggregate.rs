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
            _ if self.is_none() => {
                return Err(EventPublisherError::EventPublisherNotFound);
            }
            UpdateEventPublisher {
                name,
                description,
                filter,
                config,
            } => {
                self.update_event_publisher(name, description, filter, config, service, sink)
                    .await?;
            }
            ActivateEventPublisher => {
                self.activate_event_publisher(sink).await?;
            }
            DeactivateEventPublisher => {
                self.deactivate_event_publisher(sink).await?;
            }
            DeleteEventPublisher => {
                self.delete_event_publisher(sink).await?;
            }
        }

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
                name,
                description,
                enabled,
                filter,
                config,
                occurred_at,
            } => {
                self.event_publisher_updated(name, description, enabled, filter, config, occurred_at);
            }
            EventPublisherActivated {
                filter,
                config,
                occurred_at,
            } => {
                self.event_publisher_activated(filter, config, occurred_at);
            }
            EventPublisherDeactivated { occurred_at } => {
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
}
