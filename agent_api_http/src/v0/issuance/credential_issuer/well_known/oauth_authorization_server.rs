use crate::handlers::query_handler;
use agent_issuance::{
    server_config::views::ServerConfigView,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use std::sync::Arc;

// TODO: move this to `authorization/authorization_server/well_known.rs`!
#[axum_macros::debug_handler]
pub(crate) async fn oauth_authorization_server(State(state): State<Arc<IssuanceState>>) -> Result<Response, ApiError> {
    match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await? {
        Some(ServerConfigView {
            authorization_server_metadata,
            ..
        }) => Ok((StatusCode::OK, Json(authorization_server_metadata)).into_response()),
        _ => Err(ApiError::new(StatusCode::NOT_FOUND)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v0::issuance::router;
    use agent_issuance::state::initialize;
    use agent_secret_manager::service::Service;
    use agent_store::{in_memory::InMemory, issuance_state};
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vci::credential_issuer::authorization_server_metadata::AuthorizationServerMetadata;
    use tower::Service as _;

    pub async fn oauth_authorization_server(app: &mut Router) -> AuthorizationServerMetadata {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/.well-known/oauth-authorization-server")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let authorization_server_metadata: AuthorizationServerMetadata = serde_json::from_slice(&body).unwrap();

        assert_eq!(
            authorization_server_metadata,
            AuthorizationServerMetadata {
                issuer: "https://my-domain.example.org/".parse().unwrap(),
                authorization_endpoint: Some("https://my-domain.example.org/auth/authorize".parse().unwrap()),
                token_endpoint: Some("https://my-domain.example.org/auth/token".parse().unwrap()),
                pushed_authorization_request_endpoint: Some("https://my-domain.example.org/auth/par".parse().unwrap()),
                require_pushed_authorization_requests: Some(true),
                ..Default::default()
            }
        );

        authorization_server_metadata
    }

    #[tokio::test]
    async fn test_oauth_authorization_server_endpoint() {
        let issuance_state = Arc::new(issuance_state(&InMemory, Service::default(), Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let mut app = router(issuance_state);

        let _authorization_server_metadata = oauth_authorization_server(&mut app).await;
    }
}
