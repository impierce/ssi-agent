use thiserror::Error;

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("Credential format not known: `{0}`")]
    UnknownCredentialConfiguration(serde_json::Value),
    // TODO: Remove this error when fixing: https://github.com/impierce/ssi-agent/issues/136
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
    #[error("Failed to create the VC JWT: {0}")]
    BuildVcJwtError(String),
    #[error("Unknown Credential Identifier")]
    UnknownCredentialIdentifier,
}
