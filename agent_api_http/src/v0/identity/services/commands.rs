use crate::{error::IntoApiErrorExt, extractors::RequestActor};
use agent_identity::{
    service::{command::ServiceCommand, lifecycle},
    state::{IdentityState, DOMAIN_LINKAGE_SERVICE_ID},
};
use axum::extract::State;
use http_api_problem::ApiError;
use hyper::StatusCode;
use std::sync::Arc;

#[utoipa::path(
    post,
    path = "/create-domain-linkage",
    operation_id = "create_domain_linkage",
    tags = ["Identity"],
    responses(
        (status = 204, description = "Domain linkage created"),
        (status = 400, description = "No eligible signing DID"),
        (status = 409, description = "Service already exists"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
    )
)]
pub(crate) async fn create_domain_linkage(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
) -> Result<StatusCode, ApiError> {
    lifecycle::execute(
        &state,
        actor,
        ServiceCommand::CreateDomainLinkageService {
            service_id: DOMAIN_LINKAGE_SERVICE_ID.into(),
            verification_methods: vec![],
        },
    )
    .await
    .map_err(|error| error.into_api_error())?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/reissue-domain-linkage",
    operation_id = "reissue_domain_linkage",
    tags = ["Identity"],
    responses(
        (status = 204, description = "Domain linkage credential reissued"),
        (status = 404, description = "Service not found"),
        (status = 400, description = "No eligible signing DID"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
    )
)]
pub(crate) async fn reissue_domain_linkage(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
) -> Result<StatusCode, ApiError> {
    lifecycle::execute(
        &state,
        actor,
        ServiceCommand::ReissueDomainLinkageService {
            service_id: DOMAIN_LINKAGE_SERVICE_ID.into(),
            verification_methods: vec![],
            only_if_expiring: false,
        },
    )
    .await
    .map_err(|error| error.into_api_error())?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/remove-domain-linkage",
    operation_id = "remove_domain_linkage",
    tags = ["Identity"],
    responses(
        (status = 204, description = "Domain linkage removed"),
        (status = 404, description = "Service not found"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
    )
)]
pub(crate) async fn remove_domain_linkage(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
) -> Result<StatusCode, ApiError> {
    lifecycle::execute(
        &state,
        actor,
        ServiceCommand::DeleteDomainLinkageService {
            service_id: DOMAIN_LINKAGE_SERVICE_ID.into(),
        },
    )
    .await
    .map_err(|error| error.into_api_error())?;
    Ok(StatusCode::NO_CONTENT)
}
