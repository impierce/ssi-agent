use thiserror::Error;

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("This Credential format it not supported")]
    UnsupportedCredentialFormat,
    #[error("This Credential type it not supported")]
    UnsupportedCredentialType,
    #[error("The `credentialSubject` value is invalid: {0}")]
    InvalidCredentialSubjectError(String),
    #[error("The `id` value is invalid: {0}")]
    InvalidIdentifierError(String),
    #[error("Could not find any data to be signed")]
    MissingCredentialDataError,
    #[error("Invalid expiration data: The expiration date must not exceed `9999-12-31T23:59:59`. Please provide a valid date within the supported range.")]
    InvalidExpirationDateError,
}
