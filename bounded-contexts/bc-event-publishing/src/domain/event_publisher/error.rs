use crate::domain::event_publisher::{config::InvalidPublisherConfig, services::InvalidEventTypes};

#[cfg_attr(test, derive(PartialEq, Eq))]
#[derive(Debug, Clone, thiserror::Error)]
pub enum EventPublisherError {
    #[error("Event publisher `{0}` does not exist")]
    NotFound(String),
    #[error("Event publisher with ID `{0}` already exists")]
    AlreadyExists(String),
    #[error("Event publisher name must not be empty")]
    EmptyName,
    #[error(transparent)]
    InvalidEvents(#[from] InvalidEventTypes),
    #[error(transparent)]
    InvalidConfiguration(#[from] InvalidPublisherConfig),
}
