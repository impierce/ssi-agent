use crate::handlers::query_handler;
use agent_issuance::{offer::aggregate::Offer, state::IssuanceState};
use axum::{
    extract::{Path, State},
    response::{IntoResponse as _, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use oid4vci::credential_offer::CredentialOffer;

#[axum_macros::debug_handler]
pub(crate) async fn credential_offer_uri(
    State(state): State<IssuanceState>,
    Path(offer_id): Path<String>,
) -> Result<Response, ApiError> {
    match query_handler(&offer_id, &state.query.offer).await? {
        Some(Offer {
            credential_offer: Some(CredentialOffer::CredentialOffer(credential_offer_parameters)),
            ..
        }) => Ok((StatusCode::OK, Json(credential_offer_parameters)).into_response()),
        _ => Err(ApiError::new(StatusCode::NOT_FOUND)),
    }
}
