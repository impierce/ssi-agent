use thiserror::Error;

#[derive(Error, Debug)]
pub enum OfferError {
    #[error("The Credential Offer could not be retrieved from the `credential_offer_uri`")]
    CredentialOfferByReferenceRetrievalError,
    #[error("The Credential Issuer Metadata could not be retrieved")]
    CredentialIssuerMetadataRetrievalError,
    #[error("The Credential Offer has already been accepted and cannot be rejected anymore")]
    CredentialOfferStatusNotPendingError,
    #[error("The Credential Offer is missing")]
    MissingCredentialOfferError,
    #[error("The Authorization Server Metadata could not be retrieved")]
    AuthorizationServerMetadataRetrievalError,
    #[error("The pre-authorized code is missing from the Credential Offer")]
    MissingPreAuthorizedCodeError,
    #[error("The Authorization Server Metadata is missing the `token_endpoint` parameter")]
    MissingTokenEndpointError,
    #[error("An error occurred while requesting the access token")]
    TokenResponseError,
    #[error("The Credential Offer has not been accepted yet")]
    CredentialOfferStatusNotAcceptedError,
    #[error("The Token Response is missing from the Credential Offer")]
    MissingTokenResponseError,
    #[error("The Credential Configurations are missing from the Credential Offer")]
    MissingCredentialConfigurationsError,
    #[error("The Credential Configuration is missing from the Credential Configurations")]
    MissingCredentialConfigurationError,
    #[error("An error occurred while requesting the credentials")]
    CredentialResponseError,
    #[error("Deferred Credential Responses are not supported")]
    UnsupportedDeferredCredentialResponseError,
    #[error("Batch Credential Request are not supported")]
    BatchCredentialRequestError,
    #[error("Non-JWT credentials are not supported")]
    UnsupportedCredentialFormatError,
}
