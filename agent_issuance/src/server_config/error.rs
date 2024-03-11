use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServerConfigError {
    // TODO: Remove this error once metadata is not optional anymore.
    #[error("Missing Credential Issuer Metadata")]
    MissingCredentialIssuerMetadataError,
}
