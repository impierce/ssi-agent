use crate::{
    extractors::{JsonBody, RequestActor},
    v0::events::EventsState,
};
use axum::{extract::State, http::StatusCode};
use bc_event_publishing::domain::event_publisher::command::EventPublisherCommand;
use http_api_problem::ApiError;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Caller;
use std::sync::Arc;
use utoipa::ToSchema;

/// Request payload for deleting an event publisher.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schema(as = DeleteEventPublisher)]
pub struct DeleteEventPublisherRequest {
    #[serde(deserialize_with = "super::deserialize_non_empty_id")]
    #[schema(pattern = r"^[^\x00]+$")]
    pub id: String,
}

/// Delete an Event Publisher
///
/// Permanently deletes an event publisher and stops its background worker immediately.
#[utoipa::path(
    post,
    path = "/delete-event-publisher",
    operation_id = "delete_event_publisher",
    tag = "Events",
    request_body(
        content = DeleteEventPublisherRequest,
        example = json!({ "id": "f2202574-9a5b-5ecd-a1a3-df5ca8e87354" })
    ),
    responses(
        (status = 200, description = "Event publisher deleted", body = String),
        (status = 400, description = "Malformed JSON request body"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Event publisher not found"),
        (status = 422, description = "Request body does not match the expected schema")
    )
)]
pub async fn delete_event_publisher(
    State(state): State<Arc<EventsState>>,
    RequestActor(actor): RequestActor,
    JsonBody(payload): JsonBody<DeleteEventPublisherRequest>,
) -> Result<(StatusCode, String), ApiError> {
    let caller = actor.map_or(Caller::Anonymous, Caller::Actor);

    let id = state
        .dispatch_command(caller, payload.id, EventPublisherCommand::DeleteEventPublisher.into())
        .await?;

    Ok((StatusCode::OK, id))
}
