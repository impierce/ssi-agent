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
use serde_json::json;

#[axum_macros::debug_handler]
pub(crate) async fn offers(State(state): State<HolderState>) -> Response {
    match query_handler("all_offers", &state.query.all_received_offers).await {
        Ok(Some(all_offers_view)) => (StatusCode::OK, Json(all_offers_view)).into_response(),
        Ok(None) => (StatusCode::OK, Json(json!({}))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
