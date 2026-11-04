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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event_publisher::{
        aggregate::tests::{http_config, publisher_id},
        command::EventPublisherCommand::CreateEventPublisher,
        config::{HttpPublisherConfig, InvalidPublisherConfig},
        event::EventPublisherEvent,
        services::{EventPublisherDomainServices, InvalidEventTypes, KnownEventTypes},
    };
    use rstest::rstest;
    use shared_kernel::cqrs_es::test::TestFramework;
    use tracing_test::traced_test;

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
                    target_url: "ftp://bad.example.test".to_string(),
                    headers: None,
                }),
            })
            .then_expect_error(
                InvalidPublisherConfig::InvalidTargetUrl {
                    url: "ftp://bad.example.test".to_string(),
                    reason: "scheme must be `http` or `https`".to_string(),
                }
                .into(),
            );

        let known_services = EventPublisherDomainServices::new(KnownEventTypes::with_event_types([
            "com.impierce.unicore.issuance.credential.issued",
        ]));
        TestFramework::<EventPublisher>::with(known_services)
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
}
