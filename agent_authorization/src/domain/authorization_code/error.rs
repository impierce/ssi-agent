use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuthorizationCodeError {
    #[error("Authorization code has already been redeemed")]
    RedeemedAuthorizationCodeError,
    #[error("Authorization code has expired")]
    ExpiredAuthorizationCodeError,
    #[error("Invalid client ID provided")]
    InvalidClientIdError,
    #[error("Invalid redirect URI provided")]
    InvalidRedirectUriError,
    #[error("Missing code verifier for PKCE")]
    MissingCodeVerifierError,
    #[error("Invalid code verifier provided for PKCE")]
    InvalidCodeVerifierError,
}
