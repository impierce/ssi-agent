use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ReissuanceError {
    #[error("Reissuance relation already exists")]
    AlreadyExists,
    #[error("Failed to create reissuance: {0}")]
    BuildReissuanceError(String),
}
