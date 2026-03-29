use crate::handlers::command_handler;
use agent_verification::{data_access_consent_token::{application::redeem_data_access_consent_token::RedeemDataAccessConsentTokenService, command::DataAccessConsentTokenCommand}, state::VerificationState};
use axum::{
    extract::{State, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use std::sync::Arc;

#[axum_macros::debug_handler]
pub(crate) async fn redeem_data_access_consent_token(
    State(state): State<Arc<VerificationState>>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {

    let get_data_access_consent_token_service = RedeemDataAccessConsentTokenService {};

    let data_access_consent_token = get_data_access_consent_token_service
        .redeem_data_access_consent_token(id, &state)
        .await?;

    let response = reqwest::get(public_credential_endpoint_url) // todo: improve variable names
        .await
        .map_err(|e| {
            ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Failed to Fetch Public Credential")
                .message(format!(
                    "Failed to get response from Issuer Public Credential endpoint: {e}"
                ))
                .finish()
        })?;

    let verifiable_credential = response.json::<serde_json::Value>().await.map_err(|e| {
        ApiError::builder(StatusCode::BAD_GATEWAY)
            .title("Invalid Public Credential Response")
            .message(format!("Failed to parse Issuer Public Credential response: {e}"))
            .finish()
    })?;

    Ok((
        StatusCode::OK,
        redeemed_credential,
    )
    .into_response())
}

