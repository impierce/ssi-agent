use agent_issuance::{offer::aggregate::Offer, state::IssuanceState};
use axum::{
    extract::{Path, State},
    response::{IntoResponse as _, Response},
    Extension, Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use oid4vci::credential_offer::CredentialOffer;
use shared_kernel::authorization::Actor;
use std::sync::Arc;

use crate::handlers::{query_handler, request_actor};

#[axum_macros::debug_handler]
pub(crate) async fn credential_offer_uri(
    State(state): State<Arc<IssuanceState>>,
    actor: Option<Extension<Option<Actor>>>,
    Path(offer_id): Path<String>,
) -> Result<Response, ApiError> {
    match query_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &offer_id,
        &state.query.offer,
    )
    .await?
    {
        Some(Offer {
            credential_offer: Some(CredentialOffer::CredentialOffer(credential_offer_parameters)),
            ..
        }) => Ok((StatusCode::OK, Json(credential_offer_parameters)).into_response()),
        _ => Err(ApiError::new(StatusCode::NOT_FOUND)),
    }
}
