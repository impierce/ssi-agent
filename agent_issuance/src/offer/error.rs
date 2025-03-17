use thiserror::Error;

#[derive(Error, Debug)]
pub enum OfferError {
    #[error("Credential Offer is does not exist")]
    MissingCredentialOfferError,
    #[error("Failed to send the Credential Offer to the `target_url`: {0}")]
    SendCredentialOfferError(#[source] reqwest::Error),
    #[error("Credential is missing")]
    MissingCredentialError,
    #[error("Missing `Proof` in Credential Request")]
    MissingProofError,
    #[error("Invalid `Proof` in Credential Request")]
    InvalidProofError(String),
    #[error("Missing `iss` claim in `Proof`")]
    MissingProofIssuerError,
    #[error("Grant Type `authorization_code` is not supported")]
    UnsupportedTokenRequestGrantTypeError,
    #[error("Invalid `credential_offer_uri`: {0}")]
    InvalidCredentialOfferUriError(#[source] url::ParseError),
    #[error("Invalid URL: {0}")]
    InvalidUrlError(String),
}
