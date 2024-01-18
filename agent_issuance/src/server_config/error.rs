use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServerConfigError {
    #[error("Missing Credential Issuer Metadata")]
    MissingCredentialIssuerMetadataError,
    #[error("Missing OAuth Authorization Server Metadata")]
    MissingAuthorizationServerMetadataError,
}
