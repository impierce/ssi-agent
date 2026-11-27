use crate::domain::event_publisher::command::handle::{
    create_event_publisher::CreateEventPublisherError, update_event_publisher::UpdateEventPublisherError,
};

#[cfg_attr(test, derive(PartialEq, Eq))]
#[derive(Debug, Clone, thiserror::Error)]
pub enum EventPublisherError {
    #[error("Event publisher does not exist")]
    EventPublisherNotFound,
    #[error(transparent)]
    CreateEventPublisherError(#[from] CreateEventPublisherError),
    #[error(transparent)]
    UpdateEventPublisherError(#[from] UpdateEventPublisherError),
}
