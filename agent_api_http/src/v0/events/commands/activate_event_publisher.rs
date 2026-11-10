use crate::{extractors::RequestActor, v0::events::EventsState};
use axum::{extract::State, http::StatusCode, Json};
use bc_event_publishing::domain::event_publisher::command::EventPublisherCommand;
use http_api_problem::ApiError;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Caller;
use std::sync::Arc;
use utoipa::ToSchema;

/// Request payload for activating a paused event publisher.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schema(as = ActivateEventPublisher)]
pub struct ActivateEventPublisherRequest {
    #[serde(deserialize_with = "super::deserialize_non_empty_id")]
    #[schema(pattern = r"^[^\x00]+$")]
    pub id: String,
}

/// Activate an Event Publisher
///
/// Resumes an existing event publisher and starts its background worker immediately.
#[utoipa::path(
    post,
    path = "/activate-event-publisher",
    operation_id = "activate_event_publisher",
    tag = "Events",
    request_body(
        content = ActivateEventPublisherRequest,
        example = json!({ "id": "f2202574-9a5b-5ecd-a1a3-df5ca8e87354" })
    ),
    responses(
        (status = 200, description = "Event publisher activated", body = String),
        (status = 400, description = "Malformed JSON request body"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Event publisher not found"),
        (status = 422, description = "Request body does not match the expected schema")
    )
)]
pub async fn activate_event_publisher(
    State(state): State<Arc<EventsState>>,
    RequestActor(actor): RequestActor,
    Json(payload): Json<ActivateEventPublisherRequest>,
) -> Result<(StatusCode, String), ApiError> {
    let caller = actor.map_or(Caller::Anonymous, Caller::Actor);

    let id = state
        .dispatch_command(caller, payload.id, EventPublisherCommand::ActivateEventPublisher.into())
        .await?;

    Ok((StatusCode::OK, id))
}
