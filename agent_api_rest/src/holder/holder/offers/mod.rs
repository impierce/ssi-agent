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

#[axum_macros::debug_handler]
pub(crate) async fn offers(State(state): State<HolderState>) -> Response {
    match query_handler("all_received_offers", &state.query.all_received_offers).await {
        Ok(Some(all_received_offers_view)) => {
            let all_received_offers = all_received_offers_view
                .received_offers
                .into_iter()
                .map(|(_, credential_view)| credential_view)
                .collect::<Vec<_>>();

            (StatusCode::OK, Json(all_received_offers)).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!([]))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[axum_macros::debug_handler]
pub(crate) async fn offer(State(state): State<HolderState>, Path(received_offer_id): Path<String>) -> Response {
    match query_handler(&received_offer_id, &state.query.received_offer).await {
        Ok(Some(received_offer_view)) => (StatusCode::OK, Json(received_offer_view)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
