use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuthorizationRequestError {
    #[error("Failed to create authorization request: {0}")]
    AuthorizationRequestBuilderError(#[source] anyhow::Error),
    #[error("Missing authorization request error")]
    MissingAuthorizationRequest,
    #[error("Failed to sign authorization request: {0}")]
    AuthorizationRequestSigningError(#[source] anyhow::Error),
    #[error("Invalid SIOPv2 authorization response: {0}")]
    InvalidSIOPv2AuthorizationResponse(#[source] anyhow::Error),
    #[error("Invalid OID4VP authorization response: {0}")]
    InvalidOID4VPAuthorizationResponse(#[source] anyhow::Error),
    #[error("`jwt` parameter is not supported yet")]
    UnsupportedJwtParameterError,
}
