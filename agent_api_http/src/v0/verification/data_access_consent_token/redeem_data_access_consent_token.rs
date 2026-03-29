use crate::handlers::command_handler;
use agent_verification::{
    data_access_consent_token::{
        application::redeem_data_access_consent_token::{RedeemDataAccessConsentTokenService, DataAccessEndpointResponse},
        command::DataAccessConsentTokenCommand,
    },
    state::VerificationState,
};
use axum::{
    extract::{Path, State},
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
    let data_access_consent_token_service = RedeemDataAccessConsentTokenService {};

    let (data_access_endpoint, data_access_consent_token) = data_access_consent_token_service
        .validate_data_access_consent_token(id, &state)
        .await?;

    let request_body = serde_json::json!({
        "data_access_consent_token": data_access_consent_token
    });

    let response = reqwest::Client::new()
        .post(data_access_endpoint)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Failed to Fetch Public Credential") // TODO whats the new name for public credential?
                .message(format!("Failed to get response from Issuer Data Access endpoint: {e}"))
                .finish()
        })?;

    let status = response.status();
    if status != StatusCode::OK {
        return Err(ApiError::builder(status)
            .title("Failed to Redeem Public Credential") // TODO whats the new name for public credential?
            .message(format!(
                "Issuer Data Access endpoint returned an error status: {status}"
            ))
            .finish());
    }

    let typed_response: DataAccessEndpointResponse = response
        .json::<DataAccessEndpointResponse>()
        .await
        .map_err(|e| {
            ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Invalid Response from Issuer Data Access Endpoint")
                .message(format!("Failed to parse response from Issuer Data Access endpoint: {e}"))
                .finish()
        })?;

    let redeemed_credential = data_access_consent_token_service
        .validate_data_access_endpoint_response(data_access_consent_token, typed_response)
        .await?;

    Ok((StatusCode::OK, redeemed_credential).into_response())
}
