use crate::{
    error::type_url,
    extractors::RequestActor,
    v0::events::{queries::get_event_publisher_by_id::EventPublisherDto, EventsState},
};
use axum::{extract::State, http::StatusCode, Json};
use bc_event_publishing::application::{query::EventPublishingQuery, views::EventPublishingView};
use http_api_problem::ApiError;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Caller;
use std::sync::Arc;
use utoipa::ToSchema;

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

/// List all Event Publishers
///
/// Retrieves all configured event publishers, with HTTP header values redacted.
#[utoipa::path(
    get,
    path = "/get-event-publisher-list",
    operation_id = "get_event_publisher_list",
    tag = "Events",
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
