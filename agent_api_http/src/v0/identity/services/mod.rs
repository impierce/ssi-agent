pub mod linked_vp;

use crate::extractors::RequestActor;
use crate::handlers::query_handler;
use agent_identity::{
    service::aggregate::{Service, ServiceResource},
    state::IdentityState,
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use identity_document::service::Service as DocumentService;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize, utoipa::ToSchema)]
struct ServiceResponse {
    #[serde(rename = "id")]
    service_id: String,
    /// TODO: Replace this generic object schema with a schema for `identity_document::service::Service`.
    #[schema(value_type = Option<Object>)]
    service: Option<DocumentService>,
    presentation_ids: Vec<String>,
    /// TODO: Replace this generic object schema with a schema for `DomainLinkageConfiguration`.
    #[schema(value_type = Option<Object>)]
    resource: Option<ServiceResource>,
}

impl From<Service> for ServiceResponse {
    fn from(service: Service) -> Self {
        Self {
            service_id: service.service_id,
            service: service.service,
            presentation_ids: service.presentation_ids,
            resource: service.resource,
        }
    }
}

/// List identity services
#[utoipa::path(
    get,
    path = "/services",
    operation_id = "list_identity_services",
    tags = ["Identity"],
    responses(
        (status = 200, description = "Identity services", body = [ServiceResponse]),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn services(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
) -> Result<Response, ApiError> {
    let all_services = query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        "all_services",
        None,
        &state.query.all_services,
    )
    .await?
    .map(|all_services_view| {
        all_services_view
            .services
            .into_values()
            .filter(|service| !service.is_deleted)
            .map(ServiceResponse::from)
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_services)).into_response())
}

/// Get an identity service
#[utoipa::path(
    get,
    path = "/services/{service_id}",
    operation_id = "get_identity_service",
    tags = ["Identity"],
    params(
        ("service_id" = String, Path, description = "Identity service ID"),
    ),
    responses(
        (status = 200, description = "Identity service", body = ServiceResponse),
        (status = 404, description = "Identity service not found"),
    )
)]
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
        Some(&service_id),
        &state.query.service,
    )
    .await?
    .filter(|service_view| !service_view.is_deleted)
    .map(|service_view| (StatusCode::OK, Json(ServiceResponse::from(service_view))).into_response())
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
