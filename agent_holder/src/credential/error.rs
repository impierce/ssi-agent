use thiserror::Error;

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("Failed to decode Credential JWT")]
    CredentialDecodingError,
}
