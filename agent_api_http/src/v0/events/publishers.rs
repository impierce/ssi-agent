use crate::{
    error::type_url,
    extractors::RequestActor,
    v0::events::{config::EventPublisherConfigDto, EventsState},
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use bc_event_publishing::{
    application::{
        access::{
            ACTIVATE_PUBLISHER_OPERATION, CREATE_PUBLISHER_OPERATION, DEACTIVATE_PUBLISHER_OPERATION,
            DELETE_PUBLISHER_OPERATION, READ_PUBLISHERS_OPERATION, UPDATE_PUBLISHER_OPERATION,
        },
        query::EventPublishingQuery,
        views::{event_publisher::EventPublisherView, EventPublishingView},
    },
    domain::event_publisher::{command::EventPublisherCommand, config::EventPublisherType},
};
use chrono::{DateTime, Utc};
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

/// Published read model for a configured event publisher.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = EventPublisher)]
pub struct EventPublisherDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    /// Repeats the `config` discriminator so listing and filtering needn't reach inside it.
    pub publisher_type: EventPublisherType,
    pub events: Vec<String>,
    pub config: EventPublisherConfigDto,
    pub created_at: DateTime<Utc>,
    pub last_modified_at: DateTime<Utc>,
}

impl TryFrom<EventPublisherView> for EventPublisherDto {
    type Error = ApiError;

    fn try_from(view: EventPublisherView) -> Result<Self, Self::Error> {
        let config = view.config.ok_or_else(|| {
            ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Event Publisher Missing Configuration")
                .type_url(type_url("events#event-publisher-missing-configuration"))
                .message("The projected event publisher carries no configuration")
                .finish()
        })?;

        Ok(Self {
            id: view.id,
            name: view.name,
            description: view.description,
            enabled: view.enabled,
            publisher_type: config.publisher_type(),
            events: view.events,
            config: config.into(),
            created_at: view.created_at,
            last_modified_at: view.last_modified_at,
        })
    }
}

impl TryFrom<EventPublishingView> for EventPublisherDto {
    type Error = ApiError;

    fn try_from(view: EventPublishingView) -> Result<Self, Self::Error> {
        let EventPublishingView::EventPublisher(publisher) = view else {
            return Err(ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unexpected View Type")
                .type_url(type_url("unexpected#unexpected-error"))
                .message("Expected single EventPublisher view")
                .finish());
        };
        Self::try_from(*publisher)
    }
}

/// List of configured event publishers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[schema(as = EventPublisherList)]
pub struct EventPublisherListDto(pub Vec<EventPublisherDto>);

impl TryFrom<EventPublishingView> for EventPublisherListDto {
    type Error = ApiError;

    fn try_from(view: EventPublishingView) -> Result<Self, Self::Error> {
        let EventPublishingView::EventPublisherList(views) = view else {
            return Err(ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unexpected View Type")
                .type_url(type_url("unexpected#unexpected-error"))
                .message("Expected EventPublisher list view")
                .finish());
        };

        views
            .into_iter()
            .map(EventPublisherDto::try_from)
            .collect::<Result<_, _>>()
            .map(Self)
    }
}

/// Request payload for creating a runtime event publisher.
#[skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
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
    /// CloudEvent type patterns to subscribe to (exact types, prefix globs like `com.impierce.unicore.issuance.*`, or `*`).
    pub events: Vec<String>,
    /// Transport-specific publisher configuration (`http` or `nats`).
    pub config: EventPublisherConfigDto,
}

/// Request payload for updating an existing runtime event publisher.
#[skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = UpdateEventPublisher)]
pub struct UpdateEventPublisherRequest {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[schema(value_type = Option<String>)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub events: Option<Vec<String>>,
    #[serde(default)]
    pub config: Option<EventPublisherConfigDto>,
}

/// Request payload for activating a paused event publisher.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ActivateEventPublisher)]
pub struct ActivateEventPublisherRequest {
    pub id: String,
}

/// Request payload for deactivating (pausing) an event publisher.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = DeactivateEventPublisher)]
pub struct DeactivateEventPublisherRequest {
    pub id: String,
}

/// Request payload for deleting an event publisher.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = DeleteEventPublisher)]
pub struct DeleteEventPublisherRequest {
    pub id: String,
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
    extensions(
        ("x-access-operation" = json!(CREATE_PUBLISHER_OPERATION))
    ),
    request_body(
        content = CreateEventPublisherRequest,
        example = json!({
            "name": "Production Issuance Webhook",
            "description": "Forwards issuance CloudEvents to the billing service",
            "enabled": true,
            "events": [
                "com.impierce.unicore.issuance.credential.offered",
                "com.impierce.unicore.issuance.credential.issued"
            ],
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
        (status = 409, description = "An event publisher with this ID already exists")
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
                events: payload.events,
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

/// Update an Event Publisher
///
/// Updates the name, description, subscribed event types, or transport configuration of an existing
/// event publisher. If the publisher is currently active, its background worker is replaced immediately.
#[utoipa::path(
    post,
    path = "/update-event-publisher",
    operation_id = "update_event_publisher",
    tag = "Events",
    extensions(
        ("x-access-operation" = json!(UPDATE_PUBLISHER_OPERATION))
    ),
    request_body(
        content = UpdateEventPublisherRequest,
        example = json!({
            "id": "f2202574-9a5b-5ecd-a1a3-df5ca8e87354",
            "name": "Production Issuance Webhook",
            "description": "Updated target URL and event filter",
            "events": [
                "com.impierce.unicore.issuance.*"
            ],
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
        (status = 404, description = "Event publisher not found")
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
            id.clone(),
            EventPublisherCommand::UpdateEventPublisher {
                id,
                name: payload.name,
                description: payload.description,
                events: payload.events,
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
    let id = payload.id;

    let id = state
        .dispatch_command(
            caller,
            id.clone(),
            EventPublisherCommand::ActivateEventPublisher { id }.into(),
        )
        .await?;

    Ok((StatusCode::OK, id))
}

/// Deactivate an Event Publisher
///
/// Temporarily pauses an event publisher and stops its background worker immediately without deleting its configuration.
#[utoipa::path(
    post,
    path = "/deactivate-event-publisher",
    operation_id = "deactivate_event_publisher",
    tag = "Events",
    extensions(
        ("x-access-operation" = json!(DEACTIVATE_PUBLISHER_OPERATION))
    ),
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
    let id = payload.id;

    let id = state
        .dispatch_command(
            caller,
            id.clone(),
            EventPublisherCommand::DeactivateEventPublisher { id }.into(),
        )
        .await?;

    Ok((StatusCode::OK, id))
}

/// Delete an Event Publisher
///
/// Permanently deletes an event publisher and stops its background worker immediately.
#[utoipa::path(
    post,
    path = "/delete-event-publisher",
    operation_id = "delete_event_publisher",
    tag = "Events",
    extensions(
        ("x-access-operation" = json!(DELETE_PUBLISHER_OPERATION))
    ),
    request_body(
        content = DeleteEventPublisherRequest,
        example = json!({ "id": "f2202574-9a5b-5ecd-a1a3-df5ca8e87354" })
    ),
    responses(
        (status = 200, description = "Event publisher deleted", body = String),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Event publisher not found")
    )
)]
pub async fn delete_event_publisher(
    State(state): State<Arc<EventsState>>,
    RequestActor(actor): RequestActor,
    Json(payload): Json<DeleteEventPublisherRequest>,
) -> Result<(StatusCode, String), ApiError> {
    let caller = actor.map_or(Caller::Anonymous, Caller::Actor);
    let id = payload.id;

    let id = state
        .dispatch_command(
            caller,
            id.clone(),
            EventPublisherCommand::DeleteEventPublisher { id }.into(),
        )
        .await?;

    Ok((StatusCode::OK, id))
}

/// Get Event Publisher by ID
///
/// Retrieves a single configured event publisher by its identifier, with HTTP header values redacted.
#[utoipa::path(
    get,
    path = "/get-event-publisher-by-id/{id}",
    operation_id = "get_event_publisher_by_id",
    tag = "Events",
    extensions(
        ("x-access-operation" = json!(READ_PUBLISHERS_OPERATION))
    ),
    params(
        ("id" = String, Path, description = "Event publisher ID")
    ),
    responses(
        (status = 200, description = "Event publisher retrieved successfully", body = EventPublisherDto),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Event publisher not found")
    )
)]
pub async fn get_event_publisher_by_id(
    State(state): State<Arc<EventsState>>,
    Path(id): Path<String>,
    RequestActor(actor): RequestActor,
) -> Result<Json<EventPublisherDto>, ApiError> {
    let caller = actor.map_or(Caller::Anonymous, Caller::Actor);
    let view = state
        .dispatch_query(caller, EventPublishingQuery::GetEventPublisherById { id })
        .await?;

    Ok(Json(EventPublisherDto::try_from(view)?))
}

/// List all Event Publishers
///
/// Retrieves all configured event publishers, with HTTP header values redacted.
#[utoipa::path(
    get,
    path = "/get-event-publisher-list",
    operation_id = "get_event_publisher_list",
    tag = "Events",
    extensions(
        ("x-access-operation" = json!(READ_PUBLISHERS_OPERATION))
    ),
    responses(
        (status = 200, description = "Event publisher list retrieved successfully", body = EventPublisherListDto),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden")
    )
)]
pub async fn get_event_publisher_list(
    State(state): State<Arc<EventsState>>,
    RequestActor(actor): RequestActor,
) -> Result<Json<EventPublisherListDto>, ApiError> {
    let caller = actor.map_or(Caller::Anonymous, Caller::Actor);
    let view = state
        .dispatch_query(caller, EventPublishingQuery::GetEventPublisherList)
        .await?;

    Ok(Json(EventPublisherListDto::try_from(view)?))
}
