use crate::error::IntoApiErrorExt;
use agent_holder::{
    credential::error::CredentialError, offer::error::OfferError, presentation::error::PresentationError,
};
use http_api_problem::ApiError;
use hyper::StatusCode;

impl IntoApiErrorExt for CredentialError {
    fn into_api_error(self) -> ApiError {
        let status = match self {
            CredentialError::CredentialDecodingError => StatusCode::BAD_REQUEST,
        };

        ApiError::builder(status)
            .title("Credential Error")
            .message(self.to_string())
            .source(self)
            .finish()
    }
}

impl IntoApiErrorExt for PresentationError {
    fn into_api_error(self) -> ApiError {
        let status = match self {
            // Serialization and building errors are considered server issues.
            PresentationError::SerializationError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            PresentationError::PresentationBuilderError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            // Invalid URL or missing identifier are likely caused by client input.
            PresentationError::InvalidUrlError(_) => StatusCode::BAD_REQUEST,
            PresentationError::MissingIdentifierError(_) => StatusCode::BAD_REQUEST,
            // Signing errors are treated as server errors.
            PresentationError::SigningError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        ApiError::builder(status)
            .title("Presentation Error")
            .message(self.to_string())
            .source(self)
            .finish()
    }
}

impl IntoApiErrorExt for OfferError {
    fn into_api_error(self) -> ApiError {
        let status = match self {
            OfferError::CredentialOfferByReferenceRetrievalError => StatusCode::INTERNAL_SERVER_ERROR,
            OfferError::CredentialIssuerMetadataRetrievalError => StatusCode::INTERNAL_SERVER_ERROR,
            OfferError::CredentialOfferStatusNotPendingError => StatusCode::BAD_REQUEST,
            OfferError::MissingCredentialOfferError => StatusCode::BAD_REQUEST,
            OfferError::AuthorizationServerMetadataRetrievalError => StatusCode::INTERNAL_SERVER_ERROR,
            OfferError::MissingPreAuthorizedCodeError => StatusCode::BAD_REQUEST,
            OfferError::MissingTokenEndpointError => StatusCode::INTERNAL_SERVER_ERROR,
            OfferError::TokenResponseError => StatusCode::INTERNAL_SERVER_ERROR,
            OfferError::CredentialOfferStatusNotAcceptedError => StatusCode::BAD_REQUEST,
            OfferError::MissingTokenResponseError => StatusCode::BAD_REQUEST,
            OfferError::MissingCredentialConfigurationsError => StatusCode::BAD_REQUEST,
            OfferError::MissingCredentialConfigurationError => StatusCode::BAD_REQUEST,
            OfferError::CredentialResponseError => StatusCode::INTERNAL_SERVER_ERROR,
            OfferError::UnsupportedDeferredCredentialResponseError => StatusCode::BAD_REQUEST,
            OfferError::BatchCredentialRequestError => StatusCode::BAD_REQUEST,
            OfferError::UnsupportedCredentialFormatError => StatusCode::BAD_REQUEST,
        };

        ApiError::builder(status)
            .title("Offer Error")
            .message(self.to_string())
            .source(self)
            .finish()
    }
}
