use crate::domain::event_publisher::{command::EventPublisherCommand, error::EventPublisherError};
use shared_kernel::cqrs_es::AggregateError;

#[derive(Debug)]
pub enum EventPublishingCommand {
    EventPublisher(EventPublisherCommand),
}

impl From<EventPublisherCommand> for EventPublishingCommand {
    fn from(command: EventPublisherCommand) -> Self {
        Self::EventPublisher(command)
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum EventPublishingCommandError {
    EventPublisherError(#[from] AggregateError<EventPublisherError>),
}
