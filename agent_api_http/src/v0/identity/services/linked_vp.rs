use crate::{error::IntoApiErrorExt, extractors::RequestActor};
use agent_identity::{
    service::{command::ServiceCommand, lifecycle},
    state::{IdentityState, LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID},
};
use axum::{extract::State, Json};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkedVPEndpointRequest {
    pub presentation_ids: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/create-linked-verifiable-presentation",
    operation_id = "create_linked_verifiable_presentation",
    tags = ["Identity"],
    request_body = LinkedVPEndpointRequest,
    responses(
        (status = 204, description = "Linked presentation service created"),
        (status = 409, description = "Service already exists"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
    )
)]
pub(crate) async fn create_linked_verifiable_presentation(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Json(LinkedVPEndpointRequest { presentation_ids }): Json<LinkedVPEndpointRequest>,
) -> Result<StatusCode, ApiError> {
    lifecycle::execute(
        &state,
        actor,
        ServiceCommand::CreateLinkedVerifiablePresentationService {
            service_id: LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
            presentation_ids,
        },
    )
    .await
    .map_err(|error| error.into_api_error())?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/remove-linked-verifiable-presentation",
    operation_id = "remove_linked_verifiable_presentation",
    tags = ["Identity"],
    responses(
        (status = 204, description = "Linked presentation service removed"),
        (status = 404, description = "Service not found"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
    )
)]
pub(crate) async fn remove_linked_verifiable_presentation(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
) -> Result<StatusCode, ApiError> {
    lifecycle::execute(
        &state,
        actor,
        ServiceCommand::DeleteLinkedVerifiablePresentationService {
            service_id: LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
        },
    )
    .await
    .map_err(|error| error.into_api_error())?;
    Ok(StatusCode::NO_CONTENT)
}
