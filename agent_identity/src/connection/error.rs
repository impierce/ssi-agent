use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConnectionError {
    #[error("Connection not found")]
    ConnectionNotFound,
    #[error("Failed to fetch credential issuer metadata for '{0}'")]
    CredentialIssuerMetadataFetchFailed(String),
    #[error("Domain Missing for connection '{0}'")]
    MissingDomain(String),
    #[error("Failed to fetch DID Configurations")]
    DIDResolutionFailed(String),
}
