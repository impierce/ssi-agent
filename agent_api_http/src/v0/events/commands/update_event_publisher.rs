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

/// Request payload for updating an existing runtime event publisher.
#[skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schema(as = UpdateEventPublisher)]
pub struct UpdateEventPublisherRequest {
    #[serde(deserialize_with = "super::deserialize_non_empty_id")]
    #[schema(pattern = r"^[^\x00]+$")]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[schema(value_type = Option<String>)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub filter: Option<EventPublisherFilter>,
    #[serde(default)]
    pub config: Option<EventPublisherConfigDto>,
}

/// Update an Event Publisher
///
/// Updates the name, description, event subscription filter, or transport configuration of an existing
/// event publisher. If the publisher is currently active, its background worker is replaced immediately.
#[utoipa::path(
    post,
    path = "/update-event-publisher",
    operation_id = "update_event_publisher",
    tag = "Events",
    request_body(
        content = UpdateEventPublisherRequest,
        example = json!({
            "id": "f2202574-9a5b-5ecd-a1a3-df5ca8e87354",
            "name": "Production Issuance Webhook",
            "description": "Updated target URL and event filter",
            "filter": {
                "types": [
                    "com.impierce.unicore.issuance.*"
                ]
            },
            "config": {
                "publisherType": "http",
                "targetUrl": "https://billing.example.org/v2/webhooks/unicore",
                "headers": {
                    "Authorization": "****"
                }
            }
        })
    ),
    responses(
        (status = 200, description = "Event publisher updated successfully", body = EventPublisherDto),
        (status = 400, description = "Invalid event publisher configuration or event types"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Event publisher not found"),
        (status = 422, description = "Request body does not match the expected schema")
    )
)]
pub async fn update_event_publisher(
    State(state): State<Arc<EventsState>>,
    RequestActor(actor): RequestActor,
    Json(payload): Json<UpdateEventPublisherRequest>,
) -> Result<(StatusCode, Json<EventPublisherDto>), ApiError> {
    let caller = actor.map_or(Caller::Anonymous, Caller::Actor);
    let id = payload.id;

    let id = state
        .dispatch_command(
            caller,
            id,
            EventPublisherCommand::UpdateEventPublisher {
                name: payload.name,
                description: payload.description,
                filter: payload.filter,
                config: payload.config.map(Into::into),
            }
            .into(),
        )
        .await?;

    let view = state
        .query_unchecked(EventPublishingQuery::GetEventPublisherById { id })
        .await?;

    Ok((StatusCode::OK, Json(EventPublisherDto::try_from(view)?)))
}
