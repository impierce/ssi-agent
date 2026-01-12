use agent_issuance::{nonce::command::NonceCommand, state::IssuanceState};
use agent_shared::generate_random_string;
use agent_shared::handlers::command_handler;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use axum::Json;
use http_api_problem::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NonceEndpointRequest {}

#[axum_macros::debug_handler]
pub(crate) async fn nonce(State(state): State<Arc<IssuanceState>>) -> Result<Response, ApiError> {
    let fresh_c_nonce = generate_random_string();
    let command = NonceCommand::GenerateNonce {
        c_nonce: fresh_c_nonce.clone(),
    };

    command_handler(&fresh_c_nonce, &state.command.nonce, command)
        .await
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))?;

    Ok((StatusCode::OK, Json(json!({ "c_nonce": fresh_c_nonce }))).into_response())
}
