use thiserror::Error;

#[derive(Error, Debug)]
pub enum PresentationError {
    #[error("Failed to serialize presentation: {0}")]
    SerializationError(String),
    #[error("Failed to build presentation: {0}")]
    PresentationBuilderError(String),
    #[error("Invalid URL: {0}")]
    InvalidUrlError(String),
    #[error("Missing identifier: {0}")]
    MissingIdentifierError(String),
    #[error("Failed to sign presentation: {0}")]
    SigningError(String),
}
