use thiserror::Error;

#[derive(Error, Debug)]
pub enum OfferError {
    #[error("The Credential Offer has already been accepted and cannot be rejected anymore")]
    CredentialOfferStatusNotPendingError,
    #[error("The Credential Offer has not been accepted yet")]
    CredentialOfferStatusNotAcceptedError,
}
