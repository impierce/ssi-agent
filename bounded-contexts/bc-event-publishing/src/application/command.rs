use crate::domain::event_publisher::{command::EventPublisherCommand, error::EventPublisherError};
use shared_kernel::{authorization::Actor, cqrs_es::AggregateError};

#[derive(Debug)]
pub enum EventPublishingCommand {
    EventPublisher {
        actor: Option<Actor>,
        command: EventPublisherCommand,
    },
}

impl EventPublishingCommand {
    #[must_use]
    pub fn event_publisher(actor: Option<Actor>, command: EventPublisherCommand) -> Self {
        Self::EventPublisher { actor, command }
    }
}

impl From<EventPublisherCommand> for EventPublishingCommand {
    fn from(command: EventPublisherCommand) -> Self {
        Self::event_publisher(None, command)
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum EventPublishingCommandError {
    EventPublisherError(#[from] AggregateError<EventPublisherError>),
}
