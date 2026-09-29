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
use std::sync::Arc;

use crate::handlers::public_query_handler;
use crate::v0::openapi::PROTOCOL_TAG;

/// OpenAPI representation of `identity_credential::domain_linkage::DomainLinkageConfiguration`.
///
/// See the [DID Configuration Resource](https://identity.foundation/.well-known/resources/did-configuration/#did-configuration-resource).
#[allow(dead_code)]
#[derive(utoipa::ToSchema)]
#[schema(as = DomainLinkageConfiguration)]
pub(crate) struct DomainLinkageConfigurationSchema {
    #[serde(rename = "@context")]
    #[schema(example = "https://identity.foundation/.well-known/did-configuration/v1")]
    context: String,
    /// Domain Linkage Credentials in the JSON Web Token Proof Format.
    #[schema(min_items = 1)]
    linked_dids: Vec<String>,
}

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
        (status = 200, description = "DID Configuration resource", body = DomainLinkageConfigurationSchema),
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
