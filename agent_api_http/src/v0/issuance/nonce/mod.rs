use crate::handlers::{command_handler, request_actor};
use agent_issuance::{nonce::command::NonceCommand, state::IssuanceState};
use agent_shared::generate_random_string;
use axum::{
    extract::State,
    http::{header::CACHE_CONTROL, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension,
};
use shared_kernel::authorization::Actor;

use axum::Json;
use http_api_problem::ApiError;
use serde_json::json;
use std::sync::Arc;

#[axum_macros::debug_handler]
pub(crate) async fn nonce(
    State(state): State<Arc<IssuanceState>>,
    actor: Option<Extension<Option<Actor>>>,
) -> Result<Response, ApiError> {
    let fresh_c_nonce = generate_random_string();
    let command = NonceCommand::GenerateNonce {
        c_nonce: fresh_c_nonce.clone(),
    };

    command_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &fresh_c_nonce,
        &state.command.nonce,
        command,
    )
    .await
    .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))?;

    let mut headers = HeaderMap::new();
    headers.insert(CACHE_CONTROL, "no-store".parse().unwrap());

    Ok((StatusCode::OK, headers, Json(json!({ "c_nonce": fresh_c_nonce }))).into_response())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use agent_issuance::services::IssuanceServices;
    use agent_secret_manager::service::Service;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_nonce_endpoint() {
        use crate::v0::issuance;
        use agent_store::in_memory::InMemory;
        use agent_store::issuance_state;

        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        agent_issuance::state::initialize(&issuance_state).await.unwrap();
        let issuance_app = issuance::router(issuance_state.clone());

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
