use agent_issuance::{
    handlers::query_handler,
    server_config::queries::ServerConfigView,
    state::{ApplicationState, SERVER_CONFIG_ID},
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use tracing::info;

#[axum_macros::debug_handler]
pub(crate) async fn oauth_authorization_server(State(state): State<ApplicationState>) -> impl IntoResponse {
    info!("oauth_authorization_server endpoint");
    info!("Received request");

    match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await {
        Ok(Some(ServerConfigView {
            authorization_server_metadata,
            ..
        })) => {
            info!("Returning authorization_server_metadata");
            (StatusCode::OK, Json(authorization_server_metadata)).into_response()
        }
        _ => {
            info!("Returning 500");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{app, tests::BASE_URL};

    use super::*;
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_store::in_memory;
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

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
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
        let state = in_memory::application_state().await;

        initialize(state.clone(), startup_commands(BASE_URL.clone())).await;

        let mut app = app(state);

        let _authorization_server_metadata = oauth_authorization_server(&mut app).await;
    }
}
