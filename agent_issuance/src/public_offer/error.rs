use thiserror::Error;

/// Errors that can occur during public offer operations
#[derive(Error, Debug, PartialEq)]
pub enum PublicOfferError {
    #[error("Public offer already exists")]
    AlreadyExists,
    #[error("Public offer not found")]
    NotFound,
}
