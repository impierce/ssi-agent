use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum RefreshCapabilityError {
    #[error("Refresh capability already exists")]
    AlreadyExists,
    #[error("Refresh capability does not exist")]
    NotFound,
    #[error("Refresh capability is already disabled")]
    AlreadyDisabled,
    #[error("Failed to create refresh capability: {0}")]
    BuildRefreshCapabilityError(String),
}
