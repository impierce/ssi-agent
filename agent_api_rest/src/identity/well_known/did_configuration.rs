use crate::handlers::query_handler;
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

#[axum_macros::debug_handler]
pub(crate) async fn did_configuration(State(state): State<IdentityState>) -> Result<Response, ApiError> {
    // Get the DID Configuration Resource if it exists.
    match query_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.query.service).await? {
        Some(ServiceView { is_deleted: true, .. }) => Err(ApiError::new(StatusCode::NOT_FOUND)),
        Some(ServiceView {
            resource: Some(ServiceResource::DomainLinkage(domain_linkage_configuration)),
            ..
        }) => Ok((StatusCode::OK, Json(domain_linkage_configuration)).into_response()),
        None => Err(ApiError::new(StatusCode::NOT_FOUND)),
        _ => todo!(),
    }
}
