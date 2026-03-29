use crate::error::IntoApiErrorExt;
use agent_verification::{
    data_access_consent_token::application::redeem_data_access_consent_token::{
        DataAccessEndpointResponse, RedeemDataAccessConsentTokenService,
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

/// This endpoint receives a Data Access Consent Token (DACT) ID as a path parameter.
/// It then gets the DACT from the storage and then performs several validation steps on the token.
/// When all validations pass, the credential is requested from the Issuer's Data Access endpoint.
/// The response is then validated and returned in the response along with the validation results.
/// When any validation fails, only the validation results are returned.
/// Both the Verifier and the Issuer need to perform all these checks on the Data Access Consent Token and the requested credential, zero trust is assumed.
#[axum_macros::debug_handler]
pub(crate) async fn redeem_data_access_consent_token(
    State(state): State<Arc<VerificationState>>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let mut data_access_consent_token_service = RedeemDataAccessConsentTokenService::default();

    let (data_access_endpoint, data_access_consent_token) = data_access_consent_token_service
        .validate_data_access_consent_token(id, &state)
        .await
        .map_err(|e| e.into_api_error())?;

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

    let typed_response: DataAccessEndpointResponse =
        response.json::<DataAccessEndpointResponse>().await.map_err(|e| {
            ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Invalid Response from Issuer Data Access Endpoint")
                .message(format!(
                    "Failed to parse response from Issuer Data Access endpoint: {e}"
                ))
                .finish()
        })?;

    let public_verification_response = data_access_consent_token_service
        .validate_data_access_endpoint_response(data_access_consent_token, typed_response, &state)
        .await
        .map_err(|e| e.into_api_error())?;

    Ok((StatusCode::OK, axum::Json(public_verification_response)).into_response())
}
