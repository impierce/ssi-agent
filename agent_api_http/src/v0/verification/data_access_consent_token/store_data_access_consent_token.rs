use crate::handlers::command_handler;
use agent_verification::{data_access_consent_token::command::DataAccessConsentTokenCommand, state::VerificationState};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[axum_macros::debug_handler]
pub(crate) async fn store_data_access_consent_token(
    State(state): State<Arc<VerificationState>>,
    Json(StoreDataAccessConsentTokenEndpointRequest { id, jwt }): Json<StoreDataAccessConsentTokenEndpointRequest>,
) -> Result<Response, ApiError> {
    // TODO: first go through full redeem flow to verify the token, to avoid storing malicious stuff.

    let command = DataAccessConsentTokenCommand::StoreDataAccessConsentToken {
        id: id.clone(),
        token: jwt,
    };

    command_handler(&id, &state.command.data_access_consent_token, command).await?;

    Ok((StatusCode::OK).into_response())
}

#[derive(Deserialize, Serialize)]
pub struct StoreDataAccessConsentTokenEndpointRequest {
    pub id: String,
    pub jwt: String,
}
