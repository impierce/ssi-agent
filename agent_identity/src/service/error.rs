use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServiceError {
    #[error("Missing identifier: {0}")]
    MissingIdentifierError(String),
    #[error("Invalid URL: {0}")]
    InvalidUrlError(String),
    #[error("Invalid DID: {0}")]
    InvalidDidError(String),
    #[error("Failed to build the Domain Linkage Credential: {0}")]
    DomainLinkageCredentialBuilderError(String),
    #[error("Failed to serialize credential: {0}")]
    SerializationError(String),
    #[error("Failed to sign proof: {0}")]
    SigningError(String),
    #[error("Invalid timestamp")]
    InvalidTimestampError,
    #[error("Invalid service endpoint: {0}")]
    InvalidServiceEndpointError(String),
    #[error("Error producing document: {0}")]
    ProduceDocumentError(String),
}
