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
        access::READ_PUBLISHERS_OPERATION,
        query::EventPublishingQuery,
        views::{event_publisher::EventPublisherView, EventPublishingView},
    },
    domain::event_publisher::config::{EventPublisherFilter, EventPublisherType},
};
use chrono::{DateTime, Utc};
use http_api_problem::ApiError;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use shared_kernel::authorization::Caller;
use std::sync::Arc;
use utoipa::ToSchema;

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
    pub filter: EventPublisherFilter,
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
            filter: view.filter,
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
