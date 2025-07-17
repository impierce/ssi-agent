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
                .title("Unexpected Error")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/unexpected#unexpected-error"
                ))
                .source(self)
                .finish(),
            MissingAuthorizationRequest => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unexpected Error")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/unexpected#unexpected-error"
                ))
                .source(self)
                .finish(),
            AuthorizationRequestSigningError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unexpected Error")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/unexpected#unexpected-error"
                ))
                .source(self)
                .finish(),
            SerializationError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unexpected Error")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/unexpected#unexpected-error"
                ))
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

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::error::tests::into_json_value;
    use serde_json::json;

    #[tokio::test]
    async fn verification_errors_successfully_convert_to_problem_details() {
        assert_eq!(
            into_json_value(
                AuthorizationRequestError::MissingAuthorizationRequest
                    .into_api_error()
                    .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/unexpected#unexpected-error"),
                "title": "Unexpected Error",
                "status": 500,
                "detail": "Missing Authorization Request error"
            }),
        );
    }
}
