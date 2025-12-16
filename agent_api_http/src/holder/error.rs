use crate::error::IntoApiErrorExt;
use agent_holder::{
    credential::error::CredentialError, offer::error::OfferError, presentation::error::PresentationError,
};
use http_api_problem::ApiError;
use hyper::StatusCode;

impl IntoApiErrorExt for CredentialError {
    fn into_api_error(self) -> ApiError {
        // TODO: Implement appropriate Problem Details responses
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

impl IntoApiErrorExt for PresentationError {
    fn into_api_error(self) -> ApiError {
        // TODO: Implement appropriate Problem Details responses
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

impl IntoApiErrorExt for OfferError {
    fn into_api_error(self) -> ApiError {
        // TODO: Implement appropriate Problem Details responses
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
    }
}
