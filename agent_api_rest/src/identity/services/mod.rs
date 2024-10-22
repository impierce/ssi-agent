pub mod linked_vp;

use agent_identity::state::IdentityState;
use agent_shared::handlers::query_handler;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;
use serde_json::json;

#[axum_macros::debug_handler]
pub(crate) async fn services(State(state): State<IdentityState>) -> Response {
    match query_handler("all_services", &state.query.all_services).await {
        Ok(Some(all_services_view)) => {
            let all_services = all_services_view
                .services
                .into_iter()
                .map(|(_, service_view)| service_view)
                .collect::<Vec<_>>();

            (StatusCode::OK, Json(all_services)).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!([]))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[axum_macros::debug_handler]
pub(crate) async fn service(State(state): State<IdentityState>, Path(service_id): Path<String>) -> Response {
    match query_handler(&service_id, &state.query.service).await {
        Ok(Some(service_view)) => (StatusCode::OK, Json(service_view)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
