use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuthorizationRequestError {
    #[error("Failed to create authorization request: {0}")]
    AuthorizationRequestBuilderError(#[source] anyhow::Error),
    #[error("Missing authorization request error")]
    MissingAuthorizationRequest,
    #[error("Failed to sign authorization request: {0}")]
    AuthorizationRequestSigningError(#[source] anyhow::Error),
}
