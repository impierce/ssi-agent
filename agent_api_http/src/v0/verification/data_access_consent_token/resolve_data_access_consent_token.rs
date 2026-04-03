use crate::error::IntoApiErrorExt;
use agent_verification::{
    data_access_consent_token::application::resolve_data_access_consent_token::ResolveDataAccessConsentTokenService,
    state::VerificationState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use std::sync::Arc;

/// This endpoint receives a Data Access Consent Token (DACT) ID as a path parameter.
/// It then gets the DACT from the storage and then performs several validation steps on the token.
/// When all validations pass, the credential is requested from the Issuer's Data Access endpoint.
/// The response is then validated and the Consented Credential is returned in the final response along with the validation results.
/// When any validation fails, only the validation results are returned.
/// Both the Verifier and the Issuer need to perform all these checks on the Data Access Consent Token and the requested credential, zero trust is assumed.
#[axum_macros::debug_handler]
pub(crate) async fn resolve_data_access_consent_token(
    State(state): State<Arc<VerificationState>>,
    Path(dact_id): Path<String>,
) -> Result<Response, ApiError> {
    let mut data_access_consent_token_service = ResolveDataAccessConsentTokenService::new(dact_id, None);

    let public_verification_response = data_access_consent_token_service
        .resolve_data_access_consent_token(&state)
        .await
        .map_err(|e| e.into_api_error())?;

    Ok((StatusCode::OK, Json(public_verification_response)).into_response())
}
