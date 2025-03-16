use crate::error::IntoApiErrorExt;
use agent_verification::authorization_request::error::AuthorizationRequestError;
use http_api_problem::ApiError;
use hyper::StatusCode;

impl IntoApiErrorExt for AuthorizationRequestError {
    fn into_api_error(self) -> ApiError {
        use AuthorizationRequestError::*;

        match self {
            AuthorizationRequestBuilderError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .type_url("https://docs-git-docs-problem-details-impierce.vercel.app/unicore/problem-details#authorization-request-builder-error")
                .title("Authorization Request Builder Error")
                .source(self)
                .finish(),
            MissingAuthorizationRequest => ApiError::builder(StatusCode::BAD_REQUEST)
                .type_url("https://docs-git-docs-problem-details-impierce.vercel.app/unicore/problem-details#missing-authorization-request")
                .title("Missing Authorization Request")
                .source(self)
                .finish(),
            AuthorizationRequestSigningError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .type_url("https://docs-git-docs-problem-details-impierce.vercel.app/unicore/problem-details#authorization-request-signing-error")
                .title("Authorization Request Signing Error")
                .source(self)
                .finish(),
            InvalidSIOPv2AuthorizationResponse(_) => todo!("specification API?"),
            InvalidOID4VPAuthorizationResponse(_) => todo!("specification API?"),
            UnsupportedAuthorizationResponseParameterError => todo!("specification API?"),
        }
    }
}
