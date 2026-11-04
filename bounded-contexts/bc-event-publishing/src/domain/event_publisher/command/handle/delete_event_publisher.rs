use crate::domain::event_publisher::{
    aggregate::EventPublisher, error::EventPublisherError, event::EventPublisherEvent::EventPublisherDeleted,
};
use chrono::Utc;
use shared_kernel::cqrs_es::{event_sink::EventSink, Aggregate};
use tracing::info;

impl EventPublisher {
    /// Removes an event publisher, stopping any active background delivery task.
    pub(crate) async fn delete_event_publisher(
        &mut self,
        id: String,
        sink: &EventSink<Self>,
    ) -> Result<(), <EventPublisher as Aggregate>::Error> {
        let publisher_id = match self {
            EventPublisher::Configured(inner) => inner.id.clone(),
            EventPublisher::None => return Err(EventPublisherError::NotFound(id)),
        };

        info!(publisher_id = %publisher_id, "Deleting event publisher");
        sink.write(
            EventPublisherDeleted {
                id: publisher_id,
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
        aggregate::tests::{created_http_event, publisher_id},
        command::EventPublisherCommand::{ActivateEventPublisher, DeleteEventPublisher},
        event::EventPublisherEvent,
        services::EventPublisherDomainServices,
    };
    use rstest::rstest;
    use shared_kernel::cqrs_es::test::TestFramework;
    use tracing_test::traced_test;

    #[rstest]
    #[traced_test]
    fn deleting_a_publisher_emits_deleted_and_rejects_subsequent_commands(
        publisher_id: String,
        created_http_event: EventPublisherEvent,
    ) {
        let deleted = TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(vec![created_http_event.clone()])
            .when(DeleteEventPublisher {
                id: publisher_id.clone(),
            })
            .inspect_result()
            .unwrap();
        assert_eq!(deleted.len(), 1);
        assert!(matches!(deleted[0], EventPublisherEvent::EventPublisherDeleted { .. }));

        let mut history = vec![created_http_event];
        history.extend(deleted);
        TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(history)
            .when(ActivateEventPublisher {
                id: publisher_id.clone(),
            })
            .then_expect_error(EventPublisherError::NotFound(publisher_id));
    }
}
