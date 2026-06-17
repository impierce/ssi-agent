use crate::error::{type_url, IntoApiErrorExt};
use crate::handlers::query_handler;
use agent_issuance::{
    credential::aggregate::CredentialExpiry,
    reissuance::service::{CreateReissuanceRequest, ReissuanceService},
    state::IssuanceState,
};
use axum::{
    extract::{Json, State},
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCredentialReissuanceRequest {
    pub original_credential_id: String,
    pub credential_configuration_id: String,
    pub credential: serde_json::Value,
    pub expires_at: CredentialExpiry,
    pub reason: Option<String>,
    // TODO stronger types
    pub trigger_type: Option<String>,
    pub triggered_by: Option<String>,
    pub status_action: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCredentialReissuanceResponse {
    pub id: String,
    pub original_credential_id: String,
    pub new_credential_id: String,
    pub offer_id: String,
    pub credential_configuration_id: String,
    pub credential_offer: Option<String>,
}

#[utoipa::path(
    post,
    path = "/credential-reissuance",
    operation_id = "create_credential_reissuance",
    tags = ["Issuance"],
    responses(
        (status = 201, description = "Credential reissuance prepared successfully", body = CreateCredentialReissuanceResponse)
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn create_credential_reissuance(
    State(state): State<Arc<IssuanceState>>,
    Json(CreateCredentialReissuanceRequest {
        original_credential_id,
        credential_configuration_id,
        credential,
        expires_at,
        reason,
        trigger_type,
        triggered_by,
        status_action,
    }): Json<CreateCredentialReissuanceRequest>,
) -> Result<Response, ApiError> {
    let service = ReissuanceService::default();

    let request = CreateReissuanceRequest {
        reissuance_id: uuid::Uuid::new_v4().to_string(),
        original_credential_id,
        new_credential_id: uuid::Uuid::new_v4().to_string(),
        offer_id: uuid::Uuid::new_v4().to_string(),
        credential_configuration_id,
        credential,
        expires_at,
        reason,
        trigger_type,
        triggered_by,
        status_action,
    };

    let response = service
        .create(&state, request)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    let credential_offer = query_handler(&response.offer_id, &state.query.offer)
        .await
        .map_err(|_| {
            ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Failed to query credential offer")
                .type_url(type_url("issuance#query-credential-offer-failed"))
                .message("Failed to query the prepared credential offer.")
                .finish()
        })?
        .and_then(|offer| offer.form_url_encoded_credential_offer);

    Ok((
        StatusCode::CREATED,
        Json(CreateCredentialReissuanceResponse {
            id: response.reissuance_id,
            original_credential_id: response.original_credential_id,
            new_credential_id: response.new_credential_id,
            offer_id: response.offer_id,
            credential_configuration_id: response.credential_configuration_id,
            credential_offer,
        }),
    )
        .into_response())
}
