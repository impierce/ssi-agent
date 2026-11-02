use shared_kernel::cqrs_es::persist::PersistenceError;

#[derive(Debug, Clone)]
pub enum EventPublishingQuery {
    GetEventPublisherById { id: String },
    GetEventPublisherList,
}

#[derive(Debug, thiserror::Error)]
pub enum EventPublishingQueryError {
    #[error("Event publisher query execution failed: {0}")]
    PersistenceError(#[from] PersistenceError),
    #[error("Event publisher `{0}` not found")]
    NotFound(String),
}
