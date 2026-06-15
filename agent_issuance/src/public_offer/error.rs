use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum PublicOfferError {
    #[error("Public offer already exists")]
    AlreadyExists,
    #[error("Public offer not found")]
    NotFound,
}
