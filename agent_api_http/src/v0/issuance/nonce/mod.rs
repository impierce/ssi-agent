use crate::handlers::public_command_handler;
use crate::v0::openapi::PROTOCOL_TAG;
use agent_issuance::{nonce::command::NonceCommand, state::IssuanceState};
use agent_shared::generate_random_string;
use axum::{
    extract::State,
    http::{header::CACHE_CONTROL, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use axum::Json;
use http_api_problem::ApiError;
use oid4vci::nonce_response::NonceResponse;
use std::sync::Arc;

/// Get a fresh nonce
///
/// Returns a fresh `c_nonce` to be used in key proofs, as defined by
/// [OpenID4VCI](https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-nonce-endpoint).
#[utoipa::path(
    post,
    path = "/openid4vci/nonce",
    operation_id = "openid4vci_nonce",
    tags = ["OpenID4VCI", PROTOCOL_TAG],
    responses(
        (
            status = 200,
            description = "Fresh nonce",
            body = NonceResponse,
            headers(("Cache-Control" = String, description = "Always `no-store`")),
        ),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn nonce(State(state): State<Arc<IssuanceState>>) -> Result<Response, ApiError> {
    let fresh_c_nonce = generate_random_string();
    let command = NonceCommand::GenerateNonce {
        c_nonce: fresh_c_nonce.clone(),
    };

    public_command_handler(&fresh_c_nonce, &state.command.nonce, command)
        .await
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))?;

    let mut headers = HeaderMap::new();
    headers.insert(CACHE_CONTROL, "no-store".parse().unwrap());

    Ok((StatusCode::OK, headers, Json(NonceResponse { c_nonce: fresh_c_nonce })).into_response())
}

#[cfg(test)]
pub mod tests {
    use crate::v0::issuance::{
        credentials::tests::{create_test_template, setup_library_state},
        router,
    };

    use super::*;
    use agent_issuance::services::IssuanceServices;
    use agent_secret_manager::service::Service;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_nonce_endpoint() {
        use agent_issuance::issuance_state;
        use agent_store::in_memory::InMemory;

        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        agent_issuance::state::initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        create_test_template(&library_state).await;

        let issuance_app = router((issuance_state.clone(), library_state));

        let request = Request::builder()
            .uri("/openid4vci/nonce")
            .method("POST")
            .body(Body::empty())
            .unwrap();

        let response = issuance_app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    }
}
