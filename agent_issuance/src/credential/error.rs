use thiserror::Error;

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("Credential must be an object")]
    InvalidCredentialError,

    #[error("This Credential format it not supported")]
    UnsupportedCredentialFormat,
}
