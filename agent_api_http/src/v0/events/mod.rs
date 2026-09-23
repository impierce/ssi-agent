pub mod openapi;

use axum::{
    extract::{Query, State},
    response::sse::{self, KeepAlive, Sse},
    routing::get,
    Router,
};
use chrono::{DateTime, Utc};
use futures::stream::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::json;
use shared_kernel::{
    authorization::Caller,
    event_bus::{EventBus, EventBusError, EventBusHandle, EventFilter},
};
use std::time::Duration;

use crate::error::IntoApiErrorExt;
use crate::extractors::RequestActor;
use http_api_problem::ApiError;
use shared_kernel::authorization::{
    AllowAllAuthorizationChecker, AuthorizationChecker, AuthorizationOperation, AuthorizationRequest,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct EventsState {
    pub event_bus: EventBusHandle,
    pub authorization_checker: Arc<dyn AuthorizationChecker>,
}

impl EventsState {
    #[must_use]
    pub fn new(event_bus: EventBusHandle, authorization_checker: Arc<dyn AuthorizationChecker>) -> Self {
        Self {
            event_bus,
            authorization_checker,
        }
    }
}

impl From<EventBusHandle> for EventsState {
    fn from(event_bus: EventBusHandle) -> Self {
        Self {
            event_bus,
            authorization_checker: Arc::new(AllowAllAuthorizationChecker),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct EventQueryParams {
    pub types: Option<String>,
    pub sources: Option<String>,
    pub subject: Option<String>,
    pub limit: Option<usize>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}

pub fn router(state: Arc<EventsState>) -> Router {
    Router::new().nest(
        crate::API_VERSION,
        Router::new()
            .route("/events", get(events_sse_handler))
            .with_state(state),
    )
}

/// Stream domain events as CloudEvents via SSE with Catch-Up.
#[utoipa::path(
    get,
    path = "/events",
    operation_id = "events_sse_handler",
    tag = "Events",
    extensions(
        ("x-access-operation" = json!("events.stream"))
    ),
    params(
        ("types" = Option<String>, Query, description = "Comma-separated list of CloudEvent types to filter"),
        ("sources" = Option<String>, Query, description = "Comma-separated list of sources/aggregate types to filter"),
        ("subject" = Option<String>, Query, description = "Optional aggregate/subject ID filter"),
        ("limit" = Option<usize>, Query, description = "Optional limit on historical events; defaults to 100, set to 0 for live-only stream"),
        ("since" = Option<String>, Query, description = "Filter events after RFC 3339 timestamp"),
        ("until" = Option<String>, Query, description = "Filter events before RFC 3339 timestamp")
    ),
    responses(
        (status = 200, description = "Server-Sent Events stream of CloudEvents", body = shared_kernel::event_bus::CloudEvent, content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden")
    )
)]
pub async fn events_sse_handler(
    State(state): State<Arc<EventsState>>,
    RequestActor(actor): RequestActor,
    headers: axum::http::HeaderMap,
    Query(params): Query<EventQueryParams>,
) -> Result<Sse<impl Stream<Item = Result<sse::Event, axum::Error>>>, ApiError> {
    let caller = actor.map_or(Caller::Anonymous, Caller::Actor);
    let auth_request = AuthorizationRequest {
        caller,
        operation: AuthorizationOperation::Query {
            resource_id: None,
            operation_name: "events.stream",
        },
    };

    state
        .authorization_checker
        .is_authorized(&auth_request)
        .await
        .map_err(|error| error.into_api_error())?;

    let event_bus = &state.event_bus;
    let sources: Vec<String> = params
        .sources
        .map(|sources_str| {
            sources_str
                .split(',')
                .map(|item| item.trim().to_string())
                .filter(|trimmed| !trimmed.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let event_types: Vec<String> = params
        .types
        .map(|types_str| {
            types_str
                .split(',')
                .map(|item| item.trim().to_string())
                .filter(|trimmed| !trimmed.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let filter = EventFilter {
        event_types,
        sources,
        subject: params.subject,
        since: params.since,
        until: params.until,
    };

    let last_event_id = headers
        .get("last-event-id")
        .and_then(|header_value| header_value.to_str().ok())
        .map(|id_str| id_str.to_string());

    let limit = Some(params.limit.unwrap_or(100));

    // 1. Subscribe to live events FIRST to avoid missing published events in a race condition.
    // Any events published between this subscription and the completion of history catch-up
    // will be queued in the broadcast receiver channel.
    let live_subscription = event_bus.subscribe(filter.clone());

    // 2. Query historical catch-up events.
    // If the storage reader fails, propagate an error immediately (fail-closed) so the client
    // receives an HTTP 500 rather than an incomplete stream with missing events.
    let catchup_result = event_bus
        .history_ascending(&filter, last_event_id.as_deref(), limit)
        .await
        .map_err(|error| error.into_api_error())?;

    let catchup_events = catchup_result.events;
    let mut seen_ids = std::collections::HashSet::new();

    let mut catchup_items = Vec::new();
    // 3. If Last-Event-ID was requested but not found (evicted from memory/history), signal a gap
    // so clients are informed that intermediate events were dropped.
    if catchup_result.gap_detected {
        catchup_items.push(Ok(sse::Event::default()
            .event("lagged")
            .data(json!({ "warning": "Last-Event-ID evicted from history" }).to_string())));
    }

    // 4. Stream catch-up events in chronological order, tracking IDs in seen_ids to deduplicate against the live stream.
    for cloud_event in catchup_events {
        if !seen_ids.insert(cloud_event.id.clone()) {
            continue;
        }
        let event_type = cloud_event.event_type.clone();
        let event_id = cloud_event.id.clone();
        catchup_items.push(match serde_json::to_string(&cloud_event) {
            Ok(json_data) => Ok(sse::Event::default().id(event_id).event(event_type).data(json_data)),
            Err(err) => Ok(sse::Event::default()
                .event("error")
                .data(format!("Serialization error: {}", err))),
        });
    }

    let catchup_stream = futures::stream::iter(catchup_items);

    // 5. Seamlessly transition to live stream, deduplicating any events that arrived during catch-up.
    let live_stream = live_subscription
        .filter(move |result| {
            let is_duplicate = match result {
                Ok(cloud_event) => seen_ids.remove(&cloud_event.id),
                Err(_) => false,
            };
            async move { !is_duplicate }
        })
        .map(move |result| match result {
            Ok(cloud_event) => {
                let event_type = cloud_event.event_type.clone();
                let event_id = cloud_event.id.clone();
                match serde_json::to_string(&cloud_event) {
                    Ok(json_data) => Ok(sse::Event::default().id(event_id).event(event_type).data(json_data)),
                    Err(err) => Ok(sse::Event::default()
                        .event("error")
                        .data(format!("Serialization error: {}", err))),
                }
            }
            Err(EventBusError::Lagged(dropped_count)) => Ok(sse::Event::default()
                .event("lagged")
                .data(json!({ "dropped": dropped_count }).to_string())),
            Err(err) => Ok(sse::Event::default()
                .event("error")
                .data(format!("Event bus error: {}", err))),
        });

    // 6. Chain historical catch-up stream with real-time live stream.
    let sse_stream = catchup_stream.chain(live_stream);

    Ok(Sse::new(sse_stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_events_sse_route() {
        let bus_handle = EventBusHandle::new(16);
        let cred_event = shared_kernel::event_bus::build_cloud_event(
            "credential",
            "cred-1",
            1,
            "CredentialSigned",
            serde_json::json!({"id": "cred-1"}),
            None,
        );
        let other_event = shared_kernel::event_bus::build_cloud_event(
            "other",
            "other-1",
            1,
            "OtherCreated",
            serde_json::json!({"id": "other-1"}),
            None,
        );
        bus_handle.publish(cred_event.clone());
        bus_handle.publish(other_event.clone());

        let app = router(Arc::new(bus_handle.clone().into()));

        let req = axum::http::Request::builder()
            .uri("/v0/events?sources=credential")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(response.headers().get("content-type").unwrap(), "text/event-stream");

        let mut body = response.into_body();
        let frame = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            http_body_util::BodyExt::frame(&mut body),
        )
        .await
        .ok()
        .flatten()
        .and_then(|frame_res| frame_res.ok())
        .and_then(|frame| frame.into_data().ok());

        let body_str = frame
            .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
            .unwrap_or_default();

        assert!(body_str.contains(&cred_event.id));
        assert!(!body_str.contains(&other_event.id));
    }

    #[tokio::test]
    async fn test_events_sse_catchup_route() {
        let bus_handle = EventBusHandle::new(16);
        let event1 = shared_kernel::event_bus::build_cloud_event(
            "credential",
            "cred-1",
            1,
            "CredentialSigned",
            serde_json::json!({}),
            None,
        );
        let event2 = shared_kernel::event_bus::build_cloud_event(
            "credential",
            "cred-1",
            2,
            "CredentialRevoked",
            serde_json::json!({}),
            None,
        );
        bus_handle.publish(event1.clone());
        bus_handle.publish(event2.clone());

        let app = router(Arc::new(bus_handle.clone().into()));

        let req = axum::http::Request::builder()
            .uri("/v0/events?sources=credential")
            .header("last-event-id", &event1.id)
            .body(axum::body::Body::empty())
            .unwrap();

        let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let mut body = response.into_body();
        let frame = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            http_body_util::BodyExt::frame(&mut body),
        )
        .await
        .ok()
        .flatten()
        .and_then(|frame_res| frame_res.ok())
        .and_then(|frame| frame.into_data().ok());

        let body_str = frame
            .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
            .unwrap_or_default();

        assert!(body_str.contains(&event2.id));
        assert!(!body_str.contains(&event1.id));
    }

    #[tokio::test]
    async fn test_events_sse_timestamp_filter() {
        let bus_handle = EventBusHandle::new(16);
        let now = Utc::now();

        let event = shared_kernel::event_bus::build_cloud_event(
            "credential",
            "cred-1",
            1,
            "CredentialSigned",
            serde_json::json!({}),
            Some(now),
        );
        bus_handle.publish(event.clone());

        let app = router(Arc::new(bus_handle.clone().into()));

        let past_time = (now - chrono::Duration::hours(1)).to_rfc3339();
        let uri = format!(
            "/v0/events?sources=credential&since={}",
            urlencoding::encode(&past_time)
        );

        let req = axum::http::Request::builder()
            .uri(&uri)
            .body(axum::body::Body::empty())
            .unwrap();

        let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let mut body = response.into_body();
        let frame = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            http_body_util::BodyExt::frame(&mut body),
        )
        .await
        .ok()
        .flatten()
        .and_then(|frame_res| frame_res.ok())
        .and_then(|frame| frame.into_data().ok());

        let body_str = frame
            .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
            .unwrap_or_default();

        assert!(body_str.contains(&event.id));
    }

    #[tokio::test]
    async fn test_events_sse_deduplication_between_catchup_and_live() {
        let bus_handle = EventBusHandle::new(16);
        let event1 = shared_kernel::event_bus::build_cloud_event(
            "credential",
            "cred-1",
            1,
            "CredentialSigned",
            serde_json::json!({}),
            None,
        );
        bus_handle.publish(event1.clone());

        let app = router(Arc::new(bus_handle.clone().into()));
        let req = axum::http::Request::builder()
            .uri("/v0/events?sources=credential")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let event2 = shared_kernel::event_bus::build_cloud_event(
            "credential",
            "cred-1",
            2,
            "CredentialRevoked",
            serde_json::json!({}),
            None,
        );
        bus_handle.publish(event2.clone());

        let mut body = response.into_body();

        let frame1 = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            http_body_util::BodyExt::frame(&mut body),
        )
        .await
        .ok()
        .flatten()
        .and_then(|frame_res| frame_res.ok())
        .and_then(|frame| frame.into_data().ok());

        let body1 = frame1
            .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
            .unwrap_or_default();
        assert!(body1.contains(&event1.id));

        let frame2 = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            http_body_util::BodyExt::frame(&mut body),
        )
        .await
        .ok()
        .flatten()
        .and_then(|frame_res| frame_res.ok())
        .and_then(|frame| frame.into_data().ok());

        let body2 = frame2
            .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
            .unwrap_or_default();
        assert!(body2.contains(&event2.id));
    }

    #[tokio::test]
    async fn test_events_sse_authorization() {
        struct MockAuth;
        #[async_trait::async_trait]
        impl AuthorizationChecker for MockAuth {
            async fn is_authorized(
                &self,
                request: &AuthorizationRequest,
            ) -> Result<(), shared_kernel::authorization::AuthorizationError> {
                if matches!(request.caller, Caller::Anonymous) {
                    Err(shared_kernel::authorization::AuthorizationError::Unauthorized)
                } else {
                    Ok(())
                }
            }
        }

        let bus_handle = EventBusHandle::new(16);
        let events_state = Arc::new(EventsState::new(bus_handle, Arc::new(MockAuth)));
        let app = router(events_state);

        // 1. Without actor -> 401 Unauthorized
        let req = axum::http::Request::builder()
            .uri("/v0/events")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = tower::ServiceExt::oneshot(app.clone(), req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);

        // 2. With actor -> 200 OK
        let mut req_with_actor = axum::http::Request::builder()
            .uri("/v0/events")
            .body(axum::body::Body::empty())
            .unwrap();
        req_with_actor
            .extensions_mut()
            .insert(shared_kernel::authorization::Actor {
                subject: "test-user".to_string(),
            });
        let response = tower::ServiceExt::oneshot(app, req_with_actor).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_events_sse_history_reader_error_returns_500() {
        struct FailingReader;
        #[async_trait::async_trait]
        impl shared_kernel::event_bus::EventHistoryReader for FailingReader {
            async fn history_ascending(
                &self,
                _filter: &EventFilter,
                _last_event_id: Option<&str>,
                _limit: Option<usize>,
            ) -> Result<shared_kernel::event_bus::HistoryAscendingResult, EventBusError> {
                Err(EventBusError::Source("MongoDB failure".to_string()))
            }
        }

        let bus_handle = EventBusHandle::new(16);
        bus_handle.set_history_reader(Arc::new(FailingReader));

        let app = router(Arc::new(bus_handle.into()));

        let req = axum::http::Request::builder()
            .uri("/v0/events")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }
}
