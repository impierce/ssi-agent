use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConnectionError {
    #[error("Invalid SIOPv2 authorization response: {0}")]
    InvalidSIOPv2AuthorizationResponse(#[source] anyhow::Error),
    #[error("Invalid OID4VP authorization response: {0}")]
    InvalidOID4VPAuthorizationResponse(#[source] anyhow::Error),
    #[error("`jwt` parameter is not supported yet")]
    UnsupportedJwtParameterError,
}
