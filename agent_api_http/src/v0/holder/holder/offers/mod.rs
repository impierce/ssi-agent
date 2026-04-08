pub mod accept;
pub mod reject;

use crate::handlers::query_handler;
use agent_holder::{offer::aggregate::Offer, state::HolderState};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use std::sync::Arc;

/// List all offers
///
/// Retrieves all offers received by your organisation.
#[utoipa::path(
    get,
    path = "/holder/offers",
    operation_id = "get_all_holder_credential_offers",
    tags = ["Identity", "Holder"],
    responses(
        (status = 200, description = "All offers retrieved successfully", body = [Offer]),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn offers(State(state): State<Arc<HolderState>>) -> Result<Response, ApiError> {
    let all_received_offers = query_handler("all_received_offers", &state.query.all_received_offers)
        .await?
        .map(|all_received_offers_view| {
            all_received_offers_view
                .received_offers
                .into_values()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_received_offers)).into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn offer(
    State(state): State<Arc<HolderState>>,
    Path(received_offer_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&received_offer_id, &state.query.received_offer)
        .await?
        .map(|received_offer_view| (StatusCode::OK, Json(received_offer_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
