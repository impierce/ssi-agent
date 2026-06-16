use thiserror::Error;

#[derive(Error, Debug)]
pub enum OAuth2AuthorizationRequestError {
    #[error("Failed to create OpenID4VP presentation request: {0}")]
    OpenID4VpCreationError(anyhow::Error),
    #[error("Failed to verify OpenID4VP presentation response: {0}")]
    OpenID4VpVerificationError(anyhow::Error),
}
