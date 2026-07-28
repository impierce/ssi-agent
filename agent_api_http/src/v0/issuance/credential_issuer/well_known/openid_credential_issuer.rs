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
use std::sync::Arc;

use crate::handlers::public_query_handler;

#[axum_macros::debug_handler]
pub(crate) async fn openid_credential_issuer(State(state): State<Arc<IssuanceState>>) -> Result<Response, ApiError> {
    match public_query_handler(SERVER_CONFIG_ID, &state.query.server_config).await? {
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
    use crate::{
        tests::CREDENTIAL_ISSUER_METADATA,
        v0::issuance::{
            self,
            credentials::tests::{create_test_template, setup_library_state},
        },
    };
    use agent_issuance::{services::IssuanceServices, state::initialize};
    use agent_secret_manager::service::Service;
    use agent_store::{in_memory::InMemory, issuance_state};
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
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn test_openid_credential_issuer_endpoint() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &shared_kernel::event_bus::EventBusHandle::default(), Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template(&library_state).await;

        let mut app = issuance::router((issuance_state.clone(), library_state));

        let credential_issuer_metadata = openid_credential_issuer(&mut app).await;

        assert_eq!(
            credential_issuer_metadata.credential_issuer,
            CREDENTIAL_ISSUER_METADATA.credential_issuer
        );
        assert_eq!(
            credential_issuer_metadata.credential_endpoint,
            CREDENTIAL_ISSUER_METADATA.credential_endpoint
        );
        assert!(credential_issuer_metadata
            .credential_configurations_supported
            .contains_key(&template_id));
    }
}
