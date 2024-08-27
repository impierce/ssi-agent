pub mod accept;
pub mod reject;

use agent_holder::state::HolderState;
use agent_shared::handlers::query_handler;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;

use crate::holder::TEMP_OFFER_ID;

#[axum_macros::debug_handler]
pub(crate) async fn offers(State(state): State<HolderState>) -> Response {
    // TODO: Add extension that allows for selecting all offers.
    match query_handler(TEMP_OFFER_ID, &state.query.offer).await {
        Ok(Some(offer_view)) => (StatusCode::OK, Json(offer_view)).into_response(),
        Ok(None) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
