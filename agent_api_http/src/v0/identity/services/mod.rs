pub mod linked_vp;

use crate::extractors::RequestActor;
use crate::handlers::query_handler;
use agent_identity::state::IdentityState;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use std::sync::Arc;

#[axum_macros::debug_handler]
pub(crate) async fn services(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
) -> Result<Response, ApiError> {
    let all_services = query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        "all_services",
        &state.query.all_services,
    )
    .await?
    .map(|all_services_view| all_services_view.services.into_values().collect::<Vec<_>>())
    .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_services)).into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn service(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Path(service_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &service_id,
        &state.query.service,
    )
    .await?
    .map(|service_view| (StatusCode::OK, Json(service_view)).into_response())
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
