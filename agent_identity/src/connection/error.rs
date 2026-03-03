use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConnectionError {
    #[error("Connection with ID '{0}' already exists")]
    ConnectionAlreadyExists(String),
    #[error("Connection with ID '{0}' not found")]
    ConnectionNotFound(String),
    #[error("Failed to synchronize connection with ID '{0}'")]
    ConnectionSyncFailed(String),
    #[error("Failed to fetch credential issuer metadata for '{0}'")]
    CredentialIssuerMetadataFetchFailed(String),
    #[error("Domain Missing for connection '{0}'")]
    MissingDomain(String),
    #[error("Failed to fetch DID Configurations")]
    DIDResolutionFailed(String),
}
