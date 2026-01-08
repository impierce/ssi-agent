use thiserror::Error;

#[derive(Error, Debug)]
pub enum NonceError {
    #[error("Nonce not found: `{0}`")]
    NonceNotFound(String),
    #[error("Nonce already redeemed: `{0}`")]
    NonceRedeemed(String),
    #[error("Invalid nonce: `{0}`")]
    InvalidNonce(String),
}
