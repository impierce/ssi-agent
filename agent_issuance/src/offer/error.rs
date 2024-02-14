use thiserror::Error;

#[derive(Error, Debug)]
pub enum OfferError {
    #[error("Credential is missing")]
    MissingCredentialError,
    #[error("Missing `Proof` in Credential Request")]
    MissingProofError,
    #[error("Invalid `Proof` in Credential Request")]
    InvalidProofError(String),
    #[error("Missing `iss` claim in `Proof`")]
    MissingProofIssuerError,
}
