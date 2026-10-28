use agent_identity::{
    service::{aggregate::ServiceResource, views::ServiceView},
    state::{IdentityState, LINKED_DOMAINS_SERVICE_ID},
};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use identity_kit::domain_linkage::DomainLinkageConfiguration;
use std::sync::Arc;

use crate::handlers::public_query_handler;
use crate::v0::openapi::PROTOCOL_TAG;

/// Get the DID Configuration resource
///
/// Returns the DID Configuration resource that links this domain to the DIDs of this application, as defined by
/// [Well Known DID Configuration](https://identity.foundation/.well-known/resources/did-configuration/).
#[utoipa::path(
    get,
    path = "/.well-known/did-configuration.json",
    operation_id = "well_known_did_configuration_json",
    tags = ["DID", PROTOCOL_TAG],
    responses(
        (status = 200, description = "DID Configuration resource", body = DomainLinkageConfiguration),
        (status = 404, description = "No domain is linked"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn did_configuration(State(state): State<Arc<IdentityState>>) -> Result<Response, ApiError> {
    // Get the DID Configuration Resource if it exists.
    match public_query_handler(LINKED_DOMAINS_SERVICE_ID, &state.query.service).await? {
        Some(ServiceView {
            is_deleted: false,
            resource: Some(ServiceResource::LinkedDomains(domain_linkage_configuration)),
            ..
        }) => Ok((StatusCode::OK, Json(domain_linkage_configuration)).into_response()),
        _ => Err(ApiError::new(StatusCode::NOT_FOUND)),
    }
}
