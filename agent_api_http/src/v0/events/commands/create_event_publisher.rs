use crate::{
    extractors::RequestActor,
    v0::events::{config::EventPublisherConfigDto, queries::get_event_publisher_by_id::EventPublisherDto, EventsState},
};
use axum::{extract::State, http::StatusCode, Json};
use bc_event_publishing::{
    application::query::EventPublishingQuery,
    domain::event_publisher::{command::EventPublisherCommand, config::EventPublisherFilter},
};
use http_api_problem::ApiError;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use shared_kernel::authorization::Caller;
use std::sync::Arc;
use utoipa::ToSchema;
use uuid::Uuid;

const fn default_enabled() -> bool {
    true
}

/// Request payload for creating a runtime event publisher.
#[skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schema(as = CreateEventPublisher)]
pub struct CreateEventPublisherRequest {
    /// Optional explicit identifier; a UUID v4 is generated when omitted.
    #[serde(default)]
    pub id: Option<String>,
    /// Human-readable display name for this event publisher.
    pub name: String,
    /// Optional operator description.
    #[serde(default)]
    pub description: Option<String>,
    /// Whether the event publisher should start dispatching events immediately (defaults to `true`).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Event subscription filter matching `/v0/events` query parameters (`types`).
    pub filter: EventPublisherFilter,
    /// Transport-specific publisher configuration (`http` or `nats`).
    pub config: EventPublisherConfigDto,
}

/// Create an Event Publisher
///
/// Registers a new runtime event publisher (HTTP webhook or NATS publisher). When `enabled` is
/// `true` (the default), the publisher starts receiving and forwarding matching CloudEvents immediately.
#[utoipa::path(
    post,
    path = "/create-event-publisher",
    operation_id = "create_event_publisher",
    tag = "Events",
    request_body(
        content = CreateEventPublisherRequest,
        example = json!({
            "name": "Production Issuance Webhook",
            "description": "Forwards issuance CloudEvents to the billing service",
            "enabled": true,
            "filter": {
                "types": [
                    "com.impierce.unicore.issuance.credential.offered",
                    "com.impierce.unicore.issuance.credential.issued"
                ]
            },
            "config": {
                "publisherType": "http",
                "targetUrl": "https://billing.example.org/webhooks/unicore",
                "headers": {
                    "Authorization": "Bearer secret-webhook-token"
                }
            }
        })
    ),
    responses(
        (status = 200, description = "Event publisher created successfully", body = EventPublisherDto),
        (status = 400, description = "Invalid event publisher configuration or event types"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 409, description = "An event publisher with this ID already exists"),
        (status = 422, description = "Request body does not match the expected schema")
    )
)]
pub async fn create_event_publisher(
    State(state): State<Arc<EventsState>>,
    RequestActor(actor): RequestActor,
    Json(payload): Json<CreateEventPublisherRequest>,
) -> Result<(StatusCode, Json<EventPublisherDto>), ApiError> {
    let caller = actor.map_or(Caller::Anonymous, Caller::Actor);
    let id = payload
        .id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let id = state
        .dispatch_command(
            caller,
            id.clone(),
            EventPublisherCommand::CreateEventPublisher {
                id,
                name: payload.name,
                description: payload.description,
                enabled: payload.enabled,
                filter: payload.filter,
                config: payload.config.into(),
            }
            .into(),
        )
        .await?;

    let view = state
        .query_unchecked(EventPublishingQuery::GetEventPublisherById { id })
        .await?;

    Ok((StatusCode::OK, Json(EventPublisherDto::try_from(view)?)))
}
