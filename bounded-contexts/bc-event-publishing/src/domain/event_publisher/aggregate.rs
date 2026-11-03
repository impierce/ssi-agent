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
    use crate::domain::event_publisher::{
        config::{HttpPublisherConfig, InvalidPublisherConfig, NatsPublisherConfig, REDACTED_HEADER_VALUE},
        services::InvalidEventTypes,
    };
    use rstest::{fixture, rstest};
    use shared_kernel::cqrs_es::test::TestFramework;
    use std::collections::BTreeMap;
    use tracing_test::traced_test;

    #[fixture]
    pub fn publisher_id() -> String {
        "f2202574-9a5b-5ecd-a1a3-df5ca8e87354".to_string()
    }

    #[fixture]
    pub fn http_config() -> EventPublisherConfig {
        let mut headers = BTreeMap::new();
        headers.insert("authorization".to_string(), "Bearer super-secret".to_string());
        EventPublisherConfig::Http(HttpPublisherConfig {
            target_url: "https://example.com/webhooks/unicore".to_string(),
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

    #[rstest]
    #[traced_test]
    fn creating_an_http_publisher_succeeds(publisher_id: String, http_config: EventPublisherConfig) {
        let events = TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given_no_previous_events()
            .when(CreateEventPublisher {
                id: publisher_id.clone(),
                name: " CRM Webhook ".to_string(),
                description: Some(" Forwards issuance events ".to_string()),
                enabled: true,
                filter: EventPublisherFilter {
                    types: vec!["com.impierce.unicore.issuance.credential.issued".to_string()],
                },
                config: http_config.clone(),
            })
            .inspect_result()
            .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            EventPublisherEvent::EventPublisherCreated {
                id,
                name,
                description,
                enabled,
                filter,
                config,
                ..
            } => {
                assert_eq!(id, &publisher_id);
                assert_eq!(name, "CRM Webhook");
                assert_eq!(description.as_deref(), Some("Forwards issuance events"));
                assert!(*enabled);
                assert_eq!(
                    filter,
                    &EventPublisherFilter {
                        types: vec!["com.impierce.unicore.issuance.credential.issued".to_string()],
                    }
                );
                assert_eq!(config, &http_config);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[rstest]
    #[traced_test]
    fn creating_with_invalid_url_or_unknown_event_is_rejected(publisher_id: String, http_config: EventPublisherConfig) {
        TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given_no_previous_events()
            .when(CreateEventPublisher {
                id: publisher_id.clone(),
                name: "Bad URL".to_string(),
                description: None,
                enabled: true,
                filter: EventPublisherFilter {
                    types: vec!["com.impierce.unicore.issuance.credential.issued".to_string()],
                },
                config: EventPublisherConfig::Http(HttpPublisherConfig {
                    target_url: "ftp://bad.example.com".to_string(),
                    headers: None,
                }),
            })
            .then_expect_error(
                InvalidPublisherConfig::InvalidTargetUrl {
                    url: "ftp://bad.example.com".to_string(),
                    reason: "scheme must be `http` or `https`".to_string(),
                }
                .into(),
            );

        TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given_no_previous_events()
            .when(CreateEventPublisher {
                id: publisher_id,
                name: "Bad Event".to_string(),
                description: None,
                enabled: true,
                filter: EventPublisherFilter {
                    types: vec!["com.unknown.event.type".to_string()],
                },
                config: http_config,
            })
            .then_expect_error(InvalidEventTypes::UnknownEventType("com.unknown.event.type".to_string()).into());
    }

    #[rstest]
    #[traced_test]
    fn updating_preserves_redacted_headers_and_skips_unchanged_writes(
        publisher_id: String,
        created_http_event: EventPublisherEvent,
    ) {
        let mut redacted_headers = BTreeMap::new();
        redacted_headers.insert("authorization".to_string(), REDACTED_HEADER_VALUE.to_string());

        let events = TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(vec![created_http_event.clone()])
            .when(UpdateEventPublisher {
                id: publisher_id.clone(),
                name: Some("Updated CRM Webhook".to_string()),
                description: None,
                filter: Some(EventPublisherFilter {
                    types: vec!["com.impierce.unicore.issuance.*".to_string()],
                }),
                config: Some(EventPublisherConfig::Http(HttpPublisherConfig {
                    target_url: "https://example.com/webhooks/v2".to_string(),
                    headers: Some(redacted_headers),
                })),
            })
            .inspect_result()
            .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            EventPublisherEvent::EventPublisherUpdated {
                name,
                filter,
                config: EventPublisherConfig::Http(http),
                ..
            } => {
                assert_eq!(name, "Updated CRM Webhook");
                assert_eq!(
                    filter,
                    &EventPublisherFilter {
                        types: vec!["com.impierce.unicore.issuance.*".to_string()],
                    }
                );
                assert_eq!(http.target_url, "https://example.com/webhooks/v2");
                assert_eq!(
                    http.headers.as_ref().unwrap().get("authorization").map(String::as_str),
                    Some("Bearer super-secret")
                );
            }
            other => panic!("unexpected event: {other:?}"),
        }

        // Unchanged update emits no events
        let no_events = TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(vec![created_http_event])
            .when(UpdateEventPublisher {
                id: publisher_id,
                name: Some("CRM Webhook".to_string()),
                description: None,
                filter: None,
                config: None,
            })
            .inspect_result()
            .unwrap();
        assert!(no_events.is_empty());
    }

    #[rstest]
    #[traced_test]
    fn activate_deactivate_and_delete_lifecycle(publisher_id: String, created_http_event: EventPublisherEvent) {
        // Deactivating an active publisher emits EventPublisherDeactivated
        let deactivated = TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(vec![created_http_event.clone()])
            .when(DeactivateEventPublisher {
                id: publisher_id.clone(),
            })
            .inspect_result()
            .unwrap();
        assert_eq!(deactivated.len(), 1);
        assert!(matches!(
            deactivated[0],
            EventPublisherEvent::EventPublisherDeactivated { .. }
        ));

        // Activating a deactivated publisher emits EventPublisherActivated
        let mut history = vec![created_http_event];
        history.extend(deactivated);
        let activated = TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(history.clone())
            .when(ActivateEventPublisher {
                id: publisher_id.clone(),
            })
            .inspect_result()
            .unwrap();
        assert_eq!(activated.len(), 1);
        assert!(matches!(
            activated[0],
            EventPublisherEvent::EventPublisherActivated { .. }
        ));

        // Deleting emits EventPublisherDeleted, and subsequent commands fail with NotFound
        history.extend(activated);
        let deleted = TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(history.clone())
            .when(DeleteEventPublisher {
                id: publisher_id.clone(),
            })
            .inspect_result()
            .unwrap();
        assert_eq!(deleted.len(), 1);
        assert!(matches!(deleted[0], EventPublisherEvent::EventPublisherDeleted { .. }));

        history.extend(deleted);
        TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(history)
            .when(ActivateEventPublisher {
                id: publisher_id.clone(),
            })
            .then_expect_error(EventPublisherError::NotFound(publisher_id));
    }
}
