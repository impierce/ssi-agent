use thiserror::Error;

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("Credential must be an object")]
    InvalidCredentialError,
    #[error("This Credential format it not supported")]
    UnsupportedCredentialFormat,
    #[error("The `credentialSubject` parameter is missing")]
    MissingCredentialSubjectError,
    #[error("The supplied `credentialSubject` is invalid: {0}")]
    InvalidCredentialSubjectError(String),
    #[error("The verifiable credential is invalid: {0}")]
    InvalidVerifiableCredentialError(String),
    #[error("Could not find any data to be signed")]
    MissingCredentialDataError,
    #[error("Invalid expiration data: The expiration date must not exceed `9999-12-31T23:59:59`. Please provide a valid date within the supported range.")]
    InvalidExpirationDateError,
}
