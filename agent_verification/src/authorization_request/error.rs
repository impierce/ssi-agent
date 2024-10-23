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
    #[error("`cnf` parameter must be a JWK or a `kid` string")]
    UnsupportedCnfParameterError,
    #[error("Invalid `cnf` parameter: {0}")]
    InvalidCnfParameterError(String),
    #[error("Invalid key binding")]
    InvalidKeyBindingError,
    #[error("Invalid DID URL: {0}")]
    InvalidDidUrlError(String),
    #[error("Unsupported DID method: {0}")]
    UnsupportedDidMethodError(String),
    #[error("Unable to find verification method")]
    MissingVerificationMethodError,
    #[error("No verification method key found")]
    MissingVerificationMethodKeyError,
    #[error("Invalid disclosed object: {0}")]
    InvalidDisclosedObjectError(String),
}
