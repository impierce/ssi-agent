use agent_shared::handlers::query_handler;
use agent_verification::{authorization_request::queries::AuthorizationRequestView, state::VerificationState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[axum_macros::debug_handler]
pub(crate) async fn request(
    State(verification_state): State<VerificationState>,
    Path(request_id): Path<String>,
) -> Response {
    // Return the authorization request object.
    match query_handler(&request_id, &verification_state.query.authorization_request).await {
        Ok(Some(AuthorizationRequestView {
            signed_authorization_request_object: Some(signed_authorization_request_object),
            ..
        })) => (
            StatusCode::OK,
            // TODO: set the content type to `application/jwt` also check if this is necessary for other endpoints
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

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: String = String::from_utf8(body.to_vec()).unwrap();

        let header = body.split_once('.').unwrap().0;
        assert_eq!(header, "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkI3o2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCJ9");
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_request_endpoint() {
        let issuance_state = in_memory::issuance_state().await;
        let verification_state = in_memory::verification_state(test_verification_services(), Default::default()).await;

        let mut app = app((issuance_state, verification_state));

        let form_url_encoded_authorization_request = authorization_requests(&mut app).await;

        // Extract the state from the form_url_encoded_authorization_request.
        let state = form_url_encoded_authorization_request
            .split("%2F")
            .last()
            .unwrap()
            .to_string();

        request(&mut app, state).await;
    }
}
