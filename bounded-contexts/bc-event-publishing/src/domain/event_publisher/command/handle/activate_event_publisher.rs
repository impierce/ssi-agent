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
        sink: &EventSink<Self>,
    ) -> Result<(), <EventPublisher as Aggregate>::Error> {
        let (publisher_id, already_enabled, filter, config) = match self {
            EventPublisher::Configured(inner) => (
                inner.id.clone(),
                inner.enabled,
                inner.filter.clone(),
                inner.config.clone(),
            ),
            EventPublisher::None => return Err(EventPublisherError::EventPublisherNotFound),
        };

        if already_enabled {
            info!(publisher_id = %publisher_id, "Event publisher is already active; skipping event write");
            return Ok(());
        }

        info!(publisher_id = %publisher_id, "Activating event publisher");
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
        event::{apply::event_publisher_created::tests::created_http_event, EventPublisherEvent},
        services::EventPublisherDomainServices,
    };
    use rstest::rstest;
    use shared_kernel::cqrs_es::test::TestFramework;
    use tracing_test::traced_test;

    #[rstest]
    #[traced_test]
    fn activating_a_paused_publisher_emits_activated_event(created_http_event: EventPublisherEvent) {
        let deactivated = EventPublisherEvent::EventPublisherDeactivated {
            occurred_at: Utc::now(),
        };

        let activated = TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(vec![created_http_event, deactivated])
            .when(ActivateEventPublisher)
            .inspect_result()
            .unwrap();
        assert_eq!(activated.len(), 1);
        assert!(matches!(
            activated[0],
            EventPublisherEvent::EventPublisherActivated { .. }
        ));
    }
}
