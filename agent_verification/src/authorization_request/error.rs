use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuthorizationRequestError {
    #[error("Failed to create Authorization Request: {0}")]
    AuthorizationRequestBuilderError(#[source] anyhow::Error),
    #[error("Missing Authorization Request error")]
    MissingAuthorizationRequest,
    #[error("Failed to sign Authorization Request: {0}")]
    AuthorizationRequestSigningError(#[source] anyhow::Error),
    #[error("Invalid SIOPv2 Authorization Response: {0}")]
    InvalidSIOPv2AuthorizationResponse(#[source] anyhow::Error),
    #[error("Invalid OID4VP Authorization Response: {0}")]
    InvalidOID4VPAuthorizationResponse(#[source] anyhow::Error),
    #[error("Authorization Responses containing `response` field are not supported yet")]
    UnsupportedAuthorizationResponseParameterError,
    #[error("Failed to deserialize Authorization Request: {0}")]
    SerializationError(#[source] anyhow::Error),
}
