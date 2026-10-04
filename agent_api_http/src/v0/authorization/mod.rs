// Endpoint handlers
pub mod authorization_server;

pub mod error;
pub mod openapi;

use crate::v0::authorization::authorization_server::consent::{get_consent, post_consent};
use crate::API_VERSION;
use agent_authorization::state::AuthorizationState;
use agent_issuance::state::IssuanceState;
use authorization_server::{authorize::authorize, par::par, token::token};
use axum::routing::get;
use axum::{routing::post, Router};
use std::sync::Arc;

pub fn router((authorization_state, issuance_state): (Arc<AuthorizationState>, Arc<IssuanceState>)) -> Router {
    Router::new()
        .nest(API_VERSION, Router::new())
        .route("/auth/consent", get(get_consent).post(post_consent))
        .route("/auth/par", post(par))
        .route("/auth/authorize", get(authorize))
        .with_state(authorization_state.clone())
        .route("/auth/token", post(token))
        // The `state` below only applies to the `/auth/token` endpoint where the Pre-Authorized Code flow still requires a shared state with the Issuance Bounded Context.
        .with_state((authorization_state, issuance_state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_authorization::services::AuthorizationServices;
    use agent_issuance::services::IssuanceServices;
    use agent_secret_manager::service::Service as _;
    use agent_store::{authorization_state, in_memory::InMemory, issuance_state};
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
    };
    use rstest::rstest;
    use serde_json::{json, Value};
    use tower::Service as _;

    #[rstest]
    #[case::authorize_unknown_request(http::Method::GET, "/auth/authorize?client_id=&request_uri=", None)]
    #[case::get_consent_unknown_request(http::Method::GET, "/auth/consent?request_uri=", None)]
    #[case::post_consent_unknown_request(
        http::Method::POST,
        "/auth/consent",
        Some("client_id=&request_uri=&consent_given=true")
    )]
    #[case::par_follow_up_without_openid4vp_response(http::Method::POST, "/auth/par", Some("auth_session="))]
    #[case::par_follow_up_unknown_auth_session(
        http::Method::POST,
        "/auth/par",
        Some("auth_session=unknown&openid4vp_response=%7B%7D")
    )]
    #[serial_test::serial]
    #[tokio::test]
    async fn invalid_authorization_requests_are_rejected_with_invalid_request(
        #[case] method: http::Method,
        #[case] uri: &str,
        #[case] form: Option<&str>,
    ) {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        let authorization_state = Arc::new(
            authorization_state(
                &InMemory,
                AuthorizationServices::default().await,
                &Default::default(),
                Default::default(),
            )
            .await,
        );
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();

        let mut app = router((authorization_state, issuance_state));

        let request = Request::builder().method(method).uri(uri);
        let request = match form {
            Some(form) => request
                .header(
                    http::header::CONTENT_TYPE,
                    mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                )
                .body(Body::from(form.to_string())),
            None => request.body(Body::empty()),
        }
        .unwrap();

        let response = app.call(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body, json!({ "error": "invalid_request" }));
    }
}
