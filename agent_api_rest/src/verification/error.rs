use crate::error::IntoApiErrorExt;
use agent_verification::authorization_request::error::AuthorizationRequestError;
use http_api_problem::ApiError;
use hyper::StatusCode;

impl IntoApiErrorExt for AuthorizationRequestError {
    fn into_api_error(self) -> ApiError {
        let status = match self {
            // Errors during creation or signing are internal failures.
            AuthorizationRequestError::AuthorizationRequestBuilderError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AuthorizationRequestError::MissingAuthorizationRequest => StatusCode::INTERNAL_SERVER_ERROR,
            AuthorizationRequestError::AuthorizationRequestSigningError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            // If the response provided by the client is invalid, that's a client error.
            AuthorizationRequestError::InvalidSIOPv2AuthorizationResponse(_) => StatusCode::BAD_REQUEST,
            AuthorizationRequestError::InvalidOID4VPAuthorizationResponse(_) => StatusCode::BAD_REQUEST,
            // Unsupported parameters indicate a feature not yet implemented.
            AuthorizationRequestError::UnsupportedJwtParameterError => StatusCode::NOT_IMPLEMENTED,
        };

        ApiError::builder(status)
            .title("Authorization Request Error")
            .message(self.to_string())
            .source(self)
            .finish()
    }
}
