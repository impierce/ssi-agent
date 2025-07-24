use thiserror::Error;

#[derive(Error, Debug)]
pub enum CredentialError {
    // TODO: Remove this error when fixing: https://github.com/impierce/ssi-agent/issues/136
    #[error("Credential format not supported: `{0}`")]
    UnsupportedCredentialFormat(serde_json::Value),
    // TODO: Remove this error when fixing: https://github.com/impierce/ssi-agent/issues/136
    #[error("This Credential type is not supported")]
    UnsupportedCredentialType,
    #[error("The `credentialSubject` value is invalid: {0}")]
    InvalidCredentialSubjectError(String),
    #[error("The `id` value could not be parsed to a valid URI")]
    InvalidIdentifierError,
    #[error("Could not find any data to be signed")]
    MissingCredentialDataError,
    #[error("Invalid expiration data: The expiration date must not exceed `9999-12-31T23:59:59Z`. Please provide a valid date within the supported range.")]
    InvalidExpirationDateError,
    #[error("Unable to create the `credentialStatus`")]
    InvalidCredentialStatus,
}
