use thiserror::Error;

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("Failed to decode the credential as a JWT or a `vc+sd-jwt` credential")]
    CredentialDecodingError,
    #[error("Credential invalid: {0}")]
    InvalidCredential(String),
    #[error("Credential status invalid")]
    InvalidCredentialStatus,
}
