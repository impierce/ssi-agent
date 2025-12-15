use jsonwebtoken::Algorithm;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum KeyError {
    #[error("Alias not found: {0}")]
    AliasNotFound(String),
    #[error("Alias already assigned: {0}")]
    AliasAssigned(String),
    #[error("Alias exceeds maximum length of 36 characters")]
    AliasTooLongError(String),
    #[error("Alias contains invalid characters: {0}")]
    AliasInvalidFormat(String),
    #[error("Unsupported signature algorithm: {0:?}")]
    UnsupportedSignatureAlgorithmError(Algorithm),
    // TODO: Add more specific errors as needed
}
