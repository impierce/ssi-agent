use thiserror::Error;

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("Credential must be an object")]
    InvalidCredentialError,

    #[error("This Credential format it not supported")]
    UnsupportedCredentialFormat,

    #[error("The supplied `credentialSubject` is missing or invalid")]
    MissingOrInvalidCredentialSubjectError,

    #[error("The verifiable credential is invalid: {0}")]
    InvalidVerifiableCredentialError(String),
}
