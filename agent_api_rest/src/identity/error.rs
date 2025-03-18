use crate::error::IntoApiErrorExt;
use agent_identity::{
    connection::error::ConnectionError, document::error::DocumentError, service::error::ServiceError,
};
use http_api_problem::ApiError;
use hyper::StatusCode;

impl IntoApiErrorExt for ConnectionError {
    fn into_api_error(self) -> ApiError {
        match self {}
    }
}

impl IntoApiErrorExt for DocumentError {
    fn into_api_error(self) -> ApiError {
        // TODO: Implement appropriate Problem Details responses
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

impl IntoApiErrorExt for ServiceError {
    fn into_api_error(self) -> ApiError {
        // TODO: Implement appropriate Problem Details responses
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
    }
}
