use crate::handlers::query_handler;
use agent_issuance::{
    server_config::views::ServerConfigView,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use agent_shared::config::config;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use serde_json::json;

#[axum_macros::debug_handler]
pub(crate) async fn openid_credential_issuer(State(state): State<IssuanceState>) -> Result<Response, ApiError> {
    match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await? {
        Some(ServerConfigView {
            mut credential_issuer_metadata,
            ..
        }) => {
            // TODO: remove this once the Identity Bounded Context is the single source of truth for display data.
            // This is a temporary workaround to ensure the credential issuer metadata has the correct display information.
            credential_issuer_metadata.display = Some(config().display.clone().into_iter().map(|x| json!(x)).collect());

            Ok((StatusCode::OK, Json(credential_issuer_metadata)).into_response())
        }
        _ => Err(ApiError::new(StatusCode::NOT_FOUND)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{issuance::router, tests::CREDENTIAL_ISSUER_METADATA};
    use agent_issuance::state::initialize;
    use agent_secret_manager::service::Service;
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
    use tower::Service as _;

    pub async fn openid_credential_issuer(app: &mut Router) -> CredentialIssuerMetadata {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/.well-known/openid-credential-issuer")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let credential_issuer_metadata: CredentialIssuerMetadata = serde_json::from_slice(&body).unwrap();

        assert_eq!(credential_issuer_metadata, CREDENTIAL_ISSUER_METADATA.clone());

        credential_issuer_metadata
    }

    #[tokio::test]
    async fn test_openid_credential_issuer_endpoint() {
        let issuance_state = in_memory::issuance_state(Service::default(), Default::default()).await;
        initialize(&issuance_state).await.unwrap();

        let mut app = router(issuance_state);

        let _credential_issuer_metadata = openid_credential_issuer(&mut app).await;
    }
}
