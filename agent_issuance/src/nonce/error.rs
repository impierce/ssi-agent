use thiserror::Error;

#[derive(Error, Debug)]
pub enum NonceError {
    #[error("Nonce not found: `{0}`")]
    MissingNonceError(String),
    #[error("Nonce already redeemed: `{0}`")]
    NonceRedeemedError(String),
    #[error("Invalid nonce: `{0}`")]
    InvalidNonceError(String),
}
