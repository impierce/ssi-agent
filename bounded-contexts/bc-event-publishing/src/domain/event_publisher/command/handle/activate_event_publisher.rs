use crate::domain::event_publisher::{
    aggregate::EventPublisher, error::EventPublisherError, event::EventPublisherEvent::EventPublisherActivated,
};
use chrono::Utc;
use shared_kernel::cqrs_es::{event_sink::EventSink, Aggregate};

impl EventPublisher {
    /// Idempotent activation of an event publisher.
    pub(crate) async fn activate_event_publisher(
        &mut self,
        sink: &EventSink<Self>,
    ) -> Result<(), <EventPublisher as Aggregate>::Error> {
        let (filter, config) = match self {
            EventPublisher::Configured(inner) => {
                if inner.enabled {
                    return Ok(());
                }
                (inner.filter.clone(), inner.config.clone())
            }
            EventPublisher::None => return Err(EventPublisherError::EventPublisherNotFound),
        };

        sink.write(
            EventPublisherActivated {
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
        command::EventPublisherCommand::ActivateEventPublisher,
        config::{EventPublisherConfig, EventPublisherFilter, HttpPublisherConfig},
        event::{apply::event_publisher_created::tests::created_http_event, EventPublisherEvent},
        services::EventPublisherDomainServices,
    };
    use rstest::rstest;
    use shared_kernel::cqrs_es::test::TestFramework;
    use tracing_test::traced_test;

    #[rstest]
    #[traced_test]
    fn activating_a_paused_publisher_emits_activated_event_with_current_state(created_http_event: EventPublisherEvent) {
        let deactivated = EventPublisherEvent::EventPublisherDeactivated {
            occurred_at: Utc::now(),
        };
        let updated_filter = EventPublisherFilter {
            types: vec!["com.impierce.unicore.verification.*".to_string()],
        };
        let updated_config = EventPublisherConfig::Http(HttpPublisherConfig {
            target_url: "https://example.test/webhooks/v2".to_string(),
            headers: None,
        });
        let updated = EventPublisherEvent::EventPublisherUpdated {
            name: "Updated While Paused".to_string(),
            description: None,
            enabled: false,
            filter: updated_filter.clone(),
            config: updated_config.clone(),
            occurred_at: Utc::now(),
        };

        let activated = TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(vec![created_http_event, deactivated, updated])
            .when(ActivateEventPublisher)
            .inspect_result()
            .unwrap();
        assert_eq!(activated.len(), 1);
        assert!(matches!(
            &activated[0],
            EventPublisherEvent::EventPublisherActivated { filter, config, .. }
                if filter == &updated_filter && config == &updated_config
        ));
    }

    #[rstest]
    #[traced_test]
    fn activating_an_already_enabled_publisher_is_idempotent(created_http_event: EventPublisherEvent) {
        let events = TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(vec![created_http_event])
            .when(ActivateEventPublisher)
            .inspect_result()
            .unwrap();
        assert_eq!(events, Vec::new());
    }
}
