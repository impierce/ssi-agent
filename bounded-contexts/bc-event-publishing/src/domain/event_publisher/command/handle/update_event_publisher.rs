use crate::domain::event_publisher::{
    aggregate::EventPublisher,
    config::{EventPublisherConfig, EventPublisherFilter, InvalidPublisherConfig},
    error::EventPublisherError,
    event::EventPublisherEvent::EventPublisherUpdated,
    services::InvalidEventTypes,
};
use chrono::Utc;
use shared_kernel::cqrs_es::{event_sink::EventSink, Aggregate};
use tracing::info;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdateEventPublisherError {
    #[error("Event publisher name must not be empty")]
    EmptyName,
    #[error(transparent)]
    InvalidEvents(#[from] InvalidEventTypes),
    #[error(transparent)]
    InvalidConfiguration(#[from] InvalidPublisherConfig),
}

impl EventPublisher {
    #[allow(clippy::too_many_arguments, clippy::option_option)]
    pub(crate) async fn update_event_publisher(
        &mut self,
        name: Option<String>,
        description: Option<Option<String>>,
        filter: Option<EventPublisherFilter>,
        config: Option<EventPublisherConfig>,
        service: &<EventPublisher as Aggregate>::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), EventPublisherError> {
        let current = match self {
            EventPublisher::Configured(inner) => inner.clone(),
            EventPublisher::None => return Err(EventPublisherError::EventPublisherNotFound),
        };

        let updated_name = match name {
            Some(name) => {
                let trimmed = name.trim().to_string();
                if trimmed.is_empty() {
                    return Err(UpdateEventPublisherError::EmptyName.into());
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

        let updated_filter = match filter {
            Some(filter) => service
                .known_event_types()
                .validate_and_normalize_filter(filter)
                .map_err(UpdateEventPublisherError::from)?,
            None => current.filter.clone(),
        };

        let updated_config = match config {
            Some(config) => config
                .with_preserved_secrets(&current.config)
                .normalized()
                .map_err(UpdateEventPublisherError::from)?,
            None => current.config.clone(),
        };

        if updated_name == current.name
            && updated_description == current.description
            && updated_filter == current.filter
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
                name: updated_name,
                description: updated_description,
                enabled: current.enabled,
                filter: updated_filter,
                config: updated_config,
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
        command::EventPublisherCommand::UpdateEventPublisher,
        config::{HttpPublisherConfig, REDACTED_HEADER_VALUE},
        event::{apply::event_publisher_created::tests::created_http_event, EventPublisherEvent},
        services::EventPublisherDomainServices,
    };
    use rstest::rstest;
    use shared_kernel::cqrs_es::test::TestFramework;
    use std::collections::BTreeMap;
    use tracing_test::traced_test;

    #[rstest]
    #[traced_test]
    fn updating_preserves_redacted_headers_and_skips_unchanged_writes(created_http_event: EventPublisherEvent) {
        let mut redacted_headers = BTreeMap::new();
        redacted_headers.insert("authorization".to_string(), REDACTED_HEADER_VALUE.to_string());

        let events = TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(vec![created_http_event.clone()])
            .when(UpdateEventPublisher {
                name: Some("Updated CRM Webhook".to_string()),
                description: None,
                filter: Some(EventPublisherFilter {
                    types: vec!["com.impierce.unicore.issuance.*".to_string()],
                }),
                config: Some(EventPublisherConfig::Http(HttpPublisherConfig {
                    target_url: "https://example.test/webhooks/v2".to_string(),
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
                assert_eq!(http.target_url, "https://example.test/webhooks/v2");
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
                name: Some("CRM Webhook".to_string()),
                description: None,
                filter: None,
                config: None,
            })
            .inspect_result()
            .unwrap();
        assert!(no_events.is_empty());
    }
}
