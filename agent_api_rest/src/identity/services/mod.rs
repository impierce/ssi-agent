pub mod linked_vp;

use agent_identity::state::IdentityState;
use agent_shared::handlers::query_handler;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;
use serde_json::json;

#[axum_macros::debug_handler]
pub(crate) async fn services(State(state): State<IdentityState>) -> Response {
    match query_handler("all_services", &state.query.all_services).await {
        Ok(Some(all_services_view)) => (StatusCode::OK, Json(all_services_view)).into_response(),
        Ok(None) => (StatusCode::OK, Json(json!({}))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
