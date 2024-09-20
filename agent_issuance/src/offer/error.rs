use thiserror::Error;

#[derive(Error, Debug)]
pub enum OfferError {
    #[error("Credential Offer is missing")]
    MissingCredentialOfferError,
    #[error("Something went wrong while trying to send the Credential Offer to the `target_url`: {0}")]
    SendCredentialOfferError(String),
    #[error("Credential is missing")]
    MissingCredentialError,
    #[error("Missing `Proof` in Credential Request")]
    MissingProofError,
    #[error("Invalid `Proof` in Credential Request")]
    InvalidProofError(String),
    #[error("Missing `iss` claim in `Proof`")]
    MissingProofIssuerError,
}
