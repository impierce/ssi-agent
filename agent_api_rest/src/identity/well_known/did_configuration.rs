use agent_identity::{
    service::{aggregate::ServiceResource, views::ServiceView},
    state::{IdentityState, DOMAIN_LINKAGE_SERVICE_ID},
};
use agent_shared::handlers::query_handler;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;

#[axum_macros::debug_handler]
pub(crate) async fn did_configuration(State(state): State<IdentityState>) -> Response {
    // Get the DomainLinkageConfiguration if it exists.
    match query_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.query.service).await {
        Ok(Some(ServiceView {
            resource: Some(ServiceResource::DomainLinkage(domain_linkage_configuration)),
            ..
        })) => (StatusCode::OK, Json(domain_linkage_configuration)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
