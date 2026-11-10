use crate::{extractors::RequestActor, v0::events::EventsState};
use axum::{extract::State, http::StatusCode, Json};
use bc_event_publishing::domain::event_publisher::command::EventPublisherCommand;
use http_api_problem::ApiError;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Caller;
use std::sync::Arc;
use utoipa::ToSchema;

/// Request payload for deactivating (pausing) an event publisher.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = DeactivateEventPublisher)]
pub struct DeactivateEventPublisherRequest {
    pub id: String,
}

/// Deactivate an Event Publisher
///
/// Temporarily pauses an event publisher and stops its background worker immediately without deleting its configuration.
#[utoipa::path(
    post,
    path = "/deactivate-event-publisher",
    operation_id = "deactivate_event_publisher",
    tag = "Events",
    request_body(
        content = DeactivateEventPublisherRequest,
        example = json!({ "id": "f2202574-9a5b-5ecd-a1a3-df5ca8e87354" })
    ),
    responses(
        (status = 200, description = "Event publisher deactivated", body = String),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Event publisher not found")
    )
)]
pub async fn deactivate_event_publisher(
    State(state): State<Arc<EventsState>>,
    RequestActor(actor): RequestActor,
    Json(payload): Json<DeactivateEventPublisherRequest>,
) -> Result<(StatusCode, String), ApiError> {
    let caller = actor.map_or(Caller::Anonymous, Caller::Actor);

    let id = state
        .dispatch_command(
            caller,
            payload.id,
            EventPublisherCommand::DeactivateEventPublisher.into(),
        )
        .await?;

    Ok((StatusCode::OK, id))
}
