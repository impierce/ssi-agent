use crate::{extractors::RequestActor, v0::events::EventsState};
use axum::{extract::State, http::StatusCode, Json};
use bc_event_publishing::{
    application::access::ACTIVATE_PUBLISHER_OPERATION, domain::event_publisher::command::EventPublisherCommand,
};
use http_api_problem::ApiError;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Caller;
use std::sync::Arc;
use utoipa::ToSchema;

/// Request payload for activating a paused event publisher.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ActivateEventPublisher)]
pub struct ActivateEventPublisherRequest {
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
    extensions(
        ("x-access-operation" = json!(ACTIVATE_PUBLISHER_OPERATION))
    ),
    request_body(
        content = ActivateEventPublisherRequest,
        example = json!({ "id": "f2202574-9a5b-5ecd-a1a3-df5ca8e87354" })
    ),
    responses(
        (status = 200, description = "Event publisher activated", body = String),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Event publisher not found")
    )
)]
pub async fn activate_event_publisher(
    State(state): State<Arc<EventsState>>,
    RequestActor(actor): RequestActor,
    Json(payload): Json<ActivateEventPublisherRequest>,
) -> Result<(StatusCode, String), ApiError> {
    let caller = actor.map_or(Caller::Anonymous, Caller::Actor);

    let id = state
        .dispatch_command(
            caller,
            payload.id,
            EventPublisherCommand::ActivateEventPublisher.into(),
        )
        .await?;

    Ok((StatusCode::OK, id))
}
