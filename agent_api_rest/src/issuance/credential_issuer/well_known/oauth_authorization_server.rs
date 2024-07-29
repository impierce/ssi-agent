use agent_issuance::{
    server_config::queries::ServerConfigView,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use agent_shared::handlers::query_handler;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[axum_macros::debug_handler]
pub(crate) async fn oauth_authorization_server(State(state): State<IssuanceState>) -> Response {
    match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await {
        Ok(Some(ServerConfigView {
            authorization_server_metadata,
            ..
        })) => (StatusCode::OK, Json(authorization_server_metadata)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{app, tests::BASE_URL};

    use super::*;
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_store::in_memory;
    use agent_verification::services::test_utils::test_verification_services;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vci::credential_issuer::authorization_server_metadata::AuthorizationServerMetadata;
    use tower::Service;

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
                issuer: "https://example.com/".parse().unwrap(),
                token_endpoint: Some("https://example.com/auth/token".parse().unwrap()),
                ..Default::default()
            }
        );

        authorization_server_metadata
    }

    #[tokio::test]
    async fn test_oauth_authorization_server_endpoint() {
        let issuance_state = in_memory::issuance_state(Default::default()).await;
        let verification_state = in_memory::verification_state(test_verification_services(), Default::default()).await;
        initialize(&issuance_state, startup_commands(BASE_URL.clone())).await;

        let mut app = app((issuance_state, verification_state));

        let _authorization_server_metadata = oauth_authorization_server(&mut app).await;
    }
}
