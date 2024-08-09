use agent_shared::handlers::query_handler;
use agent_verification::{authorization_request::queries::AuthorizationRequestView, state::VerificationState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hyper::header;

/// Instead of directly embedding the Authorization Request into a QR-code or deeplink, the `Relying Party` can embed a
/// `request_uri` that points to this endpoint from where the Authorization Request Object can be retrieved.
/// As described here: https://www.rfc-editor.org/rfc/rfc9101.html#name-passing-a-request-object-by-
#[axum_macros::debug_handler]
pub(crate) async fn request(
    State(verification_state): State<VerificationState>,
    Path(request_id): Path<String>,
) -> Response {
    match query_handler(&request_id, &verification_state.query.authorization_request).await {
        Ok(Some(AuthorizationRequestView {
            signed_authorization_request_object: Some(signed_authorization_request_object),
            ..
        })) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/jwt")],
            signed_authorization_request_object,
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{app, verification::authorization_requests::tests::authorization_requests};
    use agent_issuance::services::test_utils::test_issuance_services;
    use agent_store::in_memory;
    use agent_verification::services::test_utils::test_verification_services;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use tower::Service;

    pub async fn request(app: &mut Router, state: String) {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/request/{state}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/jwt");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: String = String::from_utf8(body.to_vec()).unwrap();

        let header = body.split_once('.').unwrap().0;
        assert_eq!(header, "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9");
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_request_endpoint() {
        let issuance_state = in_memory::issuance_state(test_issuance_services(), Default::default()).await;
        let verification_state = in_memory::verification_state(test_verification_services(), Default::default()).await;
        let mut app = app((issuance_state, verification_state));

        let form_url_encoded_authorization_request = authorization_requests(&mut app, false).await;

        // Extract the state from the form_url_encoded_authorization_request.
        let state = form_url_encoded_authorization_request
            .split("%2F")
            .last()
            .unwrap()
            .to_string();

        request(&mut app, state).await;
    }
}
