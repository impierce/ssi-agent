use agent_identity::{
    service::{aggregate::ServiceResource, views::ServiceView},
    state::{IdentityState, DOMAIN_LINKAGE_SERVICE_ID},
};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use std::sync::Arc;

use crate::handlers::public_query_handler;

#[axum_macros::debug_handler]
pub(crate) async fn did_configuration(State(state): State<Arc<IdentityState>>) -> Result<Response, ApiError> {
    // Get the DID Configuration Resource if it exists.
    match public_query_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.query.service).await? {
        Some(ServiceView {
            is_deleted: false,
            resource: Some(ServiceResource::DomainLinkage(domain_linkage_configuration)),
            ..
        }) => Ok((StatusCode::OK, Json(domain_linkage_configuration)).into_response()),
        _ => Err(ApiError::new(StatusCode::NOT_FOUND)),
    }
}
