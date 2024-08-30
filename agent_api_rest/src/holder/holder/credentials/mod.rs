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
pub(crate) async fn credentials(State(state): State<HolderState>) -> Response {
    match query_handler("all_credentials", &state.query.all_credentials).await {
        Ok(Some(offer_view)) => (StatusCode::OK, Json(offer_view)).into_response(),
        Ok(None) => (StatusCode::OK, Json(json!({}))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
