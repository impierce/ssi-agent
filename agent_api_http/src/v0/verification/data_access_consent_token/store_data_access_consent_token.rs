use crate::error::IntoApiErrorExt;
use crate::handlers::command_handler;
use agent_verification::{
    data_access_consent_token::{
        application::resolve_data_access_consent_token::ResolveDataAccessConsentTokenService,
        command::DataAccessConsentTokenCommand,
    },
    state::VerificationState,
};
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
    Json(StoreDataAccessConsentTokenEndpointRequest { token_id, jwt }): Json<
        StoreDataAccessConsentTokenEndpointRequest,
    >,
) -> Result<Response, ApiError> {
    // First go through the full resolve DACT flow to validate the full flow and avoid storing malicious data.
    let mut data_access_consent_token_service =
        ResolveDataAccessConsentTokenService::new(token_id.clone(), Some(jwt.clone()));

    data_access_consent_token_service
        .resolve_data_access_consent_token(&state)
        .await
        .map_err(|e| e.into_api_error())?;

    let command = DataAccessConsentTokenCommand::StoreDataAccessConsentToken {
        id: token_id.clone(),
        token: jwt,
    };

    command_handler(&token_id, &state.command.data_access_consent_token, command).await?;

    Ok((StatusCode::OK).into_response())
}

#[derive(Deserialize, Serialize)]
pub struct StoreDataAccessConsentTokenEndpointRequest {
    pub token_id: String,
    pub jwt: String,
}
