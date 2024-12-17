pub mod accept;
pub mod reject;

use agent_holder::state::HolderState;
use agent_shared::handlers::query_handler;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;
use serde_json::json;

/// Get all offers
///
/// Retrieve all pending credential offers.
#[utoipa::path(
    get,
    path = "/holder/offers",
    tag = "Holder",
    responses(
        (status = 200, description = "Successfully retrieved all pending offers."),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn offers(State(state): State<HolderState>) -> Response {
    match query_handler("all_received_offers", &state.query.all_received_offers).await {
        Ok(Some(all_received_offers_view)) => {
            let all_received_offers = all_received_offers_view
                .received_offers
                .into_values()
                .collect::<Vec<_>>();

            (StatusCode::OK, Json(all_received_offers)).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!([]))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Get an offer by ID
///
/// Retrieve an offer for a given ID.
#[utoipa::path(
    get,
    path = "/holder/offers/{id}",
    params(
        ("id" = String, Path, description = "Unique identifier of the Offer", example = "57ea9bf4-3a50-4b34-a340-7ef969bfab12"),
    ),
    tag = "Holder",
    responses(
        (status = 200, description = "Successfully retrieved an offer."),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn offer(State(state): State<HolderState>, Path(received_offer_id): Path<String>) -> Response {
    match query_handler(&received_offer_id, &state.query.received_offer).await {
        Ok(Some(received_offer_view)) => (StatusCode::OK, Json(received_offer_view)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
