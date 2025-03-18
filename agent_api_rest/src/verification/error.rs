use crate::error::IntoApiErrorExt;
use crate::DOCUMENTATION_URL;
use agent_verification::authorization_request::error::AuthorizationRequestError;
use http_api_problem::ApiError;
use hyper::StatusCode;

impl IntoApiErrorExt for AuthorizationRequestError {
    fn into_api_error(self) -> ApiError {
        use AuthorizationRequestError::*;

        match self {
            // UniCore API Problem Details
            AuthorizationRequestBuilderError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/verification#authorization-request-builder-error"
                ))
                .title("Authorization Request Builder Error")
                .source(self)
                .finish(),
            MissingAuthorizationRequest => ApiError::builder(StatusCode::BAD_REQUEST)
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/verification#missing-authorization-request"
                ))
                .title("Missing Authorization Request")
                .source(self)
                .finish(),
            AuthorizationRequestSigningError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/verification#authorization-request-signing-error"
                ))
                .title("Authorization Request Signing Error")
                .source(self)
                .finish(),

            // Public API Errors

            // `/redirect` endpoint
            InvalidSIOPv2AuthorizationResponse(_) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            InvalidOID4VPAuthorizationResponse(_) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            UnsupportedAuthorizationResponseParameterError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}
