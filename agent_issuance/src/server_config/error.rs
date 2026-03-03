use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServerConfigError {
    #[error("Cannot update provisioned credential configuration during runtime")]
    UpdateProvisionedCredentialConfigurationError,
    #[error("Cannot remove provisioned credential configuration during runtime")]
    RemoveProvisionedCredentialConfigurationError,
    #[error("Unsupported credential format identifier: `{0}`")]
    UnsupportedCredentialFormatIdentifierError(String),
}
