use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConnectionError {
    #[error("Connection with ID '{0}' already exists")]
    ConnectionAlreadyExists(String),
    #[error("Connection with ID '{0}' not found")]
    ConnectionNotFound(String),
    #[error("Failed to synchronize connection with ID '{0}'")]
    ConnectionSyncFailed(String),
}
