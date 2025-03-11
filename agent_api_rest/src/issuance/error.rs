use crate::error::IntoApiErrorExt;
use agent_issuance::{
    credential::error::CredentialError, offer::error::OfferError, server_config::error::ServerConfigError,
};
use http_api_problem::ApiError;
use hyper::StatusCode;

impl IntoApiErrorExt for CredentialError {
    fn into_api_error(self) -> ApiError {
        let status = match self {
            CredentialError::InvalidCredentialError => StatusCode::BAD_REQUEST,
            CredentialError::UnsupportedCredentialFormat => StatusCode::NOT_IMPLEMENTED,
            CredentialError::MissingCredentialSubjectError => StatusCode::BAD_REQUEST,
            CredentialError::InvalidCredentialSubjectError(_) => StatusCode::BAD_REQUEST,
            CredentialError::InvalidVerifiableCredentialError(_) => StatusCode::BAD_REQUEST,
            CredentialError::MissingCredentialDataError => StatusCode::BAD_REQUEST,
            CredentialError::InvalidExpirationDateError => StatusCode::BAD_REQUEST,
        };

        ApiError::builder(status)
            .title("Credential Error")
            .message(self.to_string())
            .source(self)
            .finish()
    }
}

impl IntoApiErrorExt for OfferError {
    fn into_api_error(self) -> ApiError {
        let status = match self {
            // The client did not supply a required offer or credential.
            OfferError::MissingCredentialOfferError => StatusCode::BAD_REQUEST,
            OfferError::MissingCredentialError => StatusCode::BAD_REQUEST,
            // Issues with the proof provided by the client.
            OfferError::MissingProofError => StatusCode::BAD_REQUEST,
            OfferError::InvalidProofError(_) => StatusCode::BAD_REQUEST,
            OfferError::MissingProofIssuerError => StatusCode::BAD_REQUEST,
            // If sending the offer to the target URL fails, that's a server-side issue.
            OfferError::SendCredentialOfferError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            // This error indicates a feature that is not supported.
            OfferError::UnsupportedTokenRequestGrantTypeError => StatusCode::NOT_IMPLEMENTED,
        };

        ApiError::builder(status)
            .title("Offer Error")
            .message(self.to_string())
            .source(self)
            .finish()
    }
}

impl IntoApiErrorExt for ServerConfigError {
    fn into_api_error(self) -> ApiError {
        match self {}
    }
}
