pub mod linked_vp;

use crate::handlers::{query_handler, request_actor};
use agent_identity::state::IdentityState;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Extension, Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use shared_kernel::authorization::Actor;
use std::sync::Arc;

#[axum_macros::debug_handler]
pub(crate) async fn services(
    State(state): State<Arc<IdentityState>>,
    actor: Option<Extension<Option<Actor>>>,
) -> Result<Response, ApiError> {
    let all_services = query_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
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
    actor: Option<Extension<Option<Actor>>>,
    Path(service_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &service_id,
        &state.query.service,
    )
    .await?
    .map(|service_view| (StatusCode::OK, Json(service_view)).into_response())
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
