use agent_issuance::{nonce::command::NonceCommand, state::IssuanceState};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response}};,

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NonceEndpointRequest {}

#[axum_macros::debug_handler]
pub(crate) async fn nonce(State(state): State<Arc<IssuanceState>>) -> Result<Response, ApiError> {
    let fresh_c_nonce = generate_random_string();
    let command = NonceCommand::GenerateNonce { c_nonce: fresh_c_nonce.clone() };

    command_handler(&state, &state.command.nonce, command).await?;

    Ok((StatusCode::OK, Json(json!({ "c_nonce": fresh_c_nonce }))).into_response())
}
