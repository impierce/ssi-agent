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
pub struct LinkedVerifiablePresentationsRequest {
    /// Presentation IDs to publish or withdraw.
    #[schema(min_items = 1)]
    pub presentation_ids: Vec<String>,
}

/// Add linked verifiable presentations
///
/// Publishes the given presentations after the presentations, signatures, holders and credential
/// subjects have been validated. Existing IDs and duplicates are no-ops; new IDs retain request
/// order after presentations that were already published.
#[utoipa::path(
    post,
    path = "/add-linked-verifiable-presentations",
    operation_id = "add_linked_verifiable_presentations",
    tags = ["Identity"],
    request_body = LinkedVerifiablePresentationsRequest,
    responses(
        (status = 204, description = "Presentations published"),
        (status = 400, description = "Malformed JSON request body"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
        (status = 422, description = "Empty list, missing presentation, or invalid presentation"),
    )
)]
pub(crate) async fn add_linked_verifiable_presentations(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Json(LinkedVerifiablePresentationsRequest { presentation_ids }): Json<LinkedVerifiablePresentationsRequest>,
) -> Result<StatusCode, ApiError> {
    lifecycle::execute(
        &state,
        actor,
        ServiceCommand::AddLinkedVerifiablePresentations {
            service_id: LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
            presentation_ids,
        },
    )
    .await
    .map_err(|error| error.into_api_error())?;
    Ok(StatusCode::NO_CONTENT)
}

/// Remove linked verifiable presentations
///
/// Withdraws the given presentations while preserving all others in their existing order. Unknown
/// IDs and duplicates are no-ops; removing a holder's last presentation removes its DID service.
#[utoipa::path(
    post,
    path = "/remove-linked-verifiable-presentations",
    operation_id = "remove_linked_verifiable_presentations",
    tags = ["Identity"],
    request_body = LinkedVerifiablePresentationsRequest,
    responses(
        (status = 204, description = "Presentations withdrawn"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
        (status = 422, description = "Empty presentation ID list"),
    )
)]
pub(crate) async fn remove_linked_verifiable_presentations(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Json(LinkedVerifiablePresentationsRequest { presentation_ids }): Json<LinkedVerifiablePresentationsRequest>,
) -> Result<StatusCode, ApiError> {
    lifecycle::execute(
        &state,
        actor,
        ServiceCommand::RemoveLinkedVerifiablePresentations {
            service_id: LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
            presentation_ids,
        },
    )
    .await
    .map_err(|error| error.into_api_error())?;
    Ok(StatusCode::NO_CONTENT)
}
