use crate::domain::event_publisher::{
    aggregate::EventPublisher, error::EventPublisherError, event::EventPublisherEvent::EventPublisherDeleted,
};
use chrono::Utc;
use shared_kernel::cqrs_es::{event_sink::EventSink, Aggregate};

impl EventPublisher {
    /// Removes an event publisher, stopping any active background delivery task.
    pub(crate) async fn delete_event_publisher(
        &mut self,
        sink: &EventSink<Self>,
    ) -> Result<(), <EventPublisher as Aggregate>::Error> {
        if self.is_none() {
            return Err(EventPublisherError::EventPublisherNotFound);
        }

        sink.write(
            EventPublisherDeleted {
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
        command::EventPublisherCommand::{ActivateEventPublisher, DeleteEventPublisher},
        event::{apply::event_publisher_created::tests::created_http_event, EventPublisherEvent},
        services::EventPublisherDomainServices,
    };
    use rstest::rstest;
    use shared_kernel::cqrs_es::test::TestFramework;
    use tracing_test::traced_test;

    #[rstest]
    #[traced_test]
    fn deleting_a_publisher_emits_deleted_and_rejects_subsequent_commands(created_http_event: EventPublisherEvent) {
        let deleted = TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(vec![created_http_event.clone()])
            .when(DeleteEventPublisher)
            .inspect_result()
            .unwrap();
        assert_eq!(deleted.len(), 1);
        assert!(matches!(deleted[0], EventPublisherEvent::EventPublisherDeleted { .. }));

        let mut history = vec![created_http_event];
        history.extend(deleted);
        TestFramework::<EventPublisher>::with(EventPublisherDomainServices::default())
            .given(history)
            .when(ActivateEventPublisher)
            .then_expect_error(EventPublisherError::EventPublisherNotFound);
    }
}
