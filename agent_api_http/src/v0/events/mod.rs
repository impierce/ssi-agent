pub mod config;
pub mod error;
pub mod openapi;
pub mod publishers;

use axum::{
    extract::{Query, State},
    response::sse::{self, KeepAlive, Sse},
    routing::{get, post},
    Router,
};
use bc_event_publishing::application::{
    command::EventPublishingCommand, query::EventPublishingQuery, service::EventPublishingContext,
    views::EventPublishingView,
};
use chrono::{DateTime, Utc};
use futures::stream::{Stream, StreamExt};
use hyper::StatusCode;
use serde::Deserialize;
use serde_json::json;
use shared_kernel::{
    application_service::ApplicationContext as _,
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
    pub event_publishing: Option<Arc<EventPublishingContext>>,
}

impl EventsState {
    #[must_use]
    pub fn new(event_bus: EventBusHandle, authorization_checker: Arc<dyn AuthorizationChecker>) -> Self {
        Self {
            event_bus,
            authorization_checker,
            event_publishing: None,
        }
    }

    #[must_use]
    pub fn with_event_publishing(mut self, event_publishing: Arc<EventPublishingContext>) -> Self {
        self.event_publishing = Some(event_publishing);
        self
    }

    pub(crate) async fn dispatch_command(
        &self,
        caller: Caller,
        aggregate_id: String,
        command: EventPublishingCommand,
    ) -> Result<String, ApiError> {
        let context = self.event_publishing_context()?;
        let auth_request = AuthorizationRequest {
            caller: caller.clone(),
            operation: AuthorizationOperation::Command {
                aggregate_id: aggregate_id.clone(),
                resource_id: context.command_resource_id(&command),
                operation_name: context.command_operation_name(&command),
            },
        };
        self.authorization_checker
            .is_authorized(&auth_request)
            .await
            .map_err(IntoApiErrorExt::into_api_error)?;

        context
            .handle_command(&aggregate_id, command, &caller)
            .await
            .map_err(IntoApiErrorExt::into_api_error)
    }

    pub(crate) async fn dispatch_query(
        &self,
        caller: Caller,
        query: EventPublishingQuery,
    ) -> Result<EventPublishingView, ApiError> {
        let context = self.event_publishing_context()?;
        let auth_request = AuthorizationRequest {
            caller,
            operation: AuthorizationOperation::Query {
                resource_id: context.query_resource_id(&query),
                operation_name: context.query_operation_name(&query),
            },
        };
        self.authorization_checker
            .is_authorized(&auth_request)
            .await
            .map_err(IntoApiErrorExt::into_api_error)?;

        context
            .handle_query(query)
            .await
            .map_err(IntoApiErrorExt::into_api_error)
    }

    pub(crate) async fn query_unchecked(&self, query: EventPublishingQuery) -> Result<EventPublishingView, ApiError> {
        let context = self.event_publishing_context()?;
        context
            .handle_query(query)
            .await
            .map_err(IntoApiErrorExt::into_api_error)
    }

    #[allow(clippy::result_large_err)]
    fn event_publishing_context(&self) -> Result<&EventPublishingContext, ApiError> {
        self.event_publishing.as_deref().ok_or_else(|| {
            ApiError::builder(StatusCode::SERVICE_UNAVAILABLE)
                .title("Event Publishing Unavailable")
                .type_url(crate::error::type_url("events#event-publishing-unavailable"))
                .message("The event publishing service is not configured")
                .finish()
        })
    }
}

impl From<EventBusHandle> for EventsState {
    fn from(event_bus: EventBusHandle) -> Self {
        Self {
            event_bus,
            authorization_checker: Arc::new(AllowAllAuthorizationChecker),
            event_publishing: None,
        }
    }
}

/// Number of historical catch-up events returned when the client does not ask for a `limit`.
const DEFAULT_CATCHUP_LIMIT: usize = 100;

/// Upper bound on historical catch-up events per request; a larger `limit` is clamped rather than
/// rejected. Catch-up is fully materialised before anything streams, so this bounds memory.
const MAX_CATCHUP_LIMIT: usize = 1000;

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
    Router::new()
        .nest(
            crate::API_VERSION,
            Router::new()
                .route("/events", get(events_sse_handler))
                .route("/create-event-publisher", post(publishers::create_event_publisher))
                .route("/update-event-publisher", post(publishers::update_event_publisher))
                .route("/activate-event-publisher", post(publishers::activate_event_publisher))
                .route(
                    "/deactivate-event-publisher",
                    post(publishers::deactivate_event_publisher),
                )
                .route("/delete-event-publisher", post(publishers::delete_event_publisher))
                .route(
                    "/get-event-publisher-by-id/{id}",
                    get(publishers::get_event_publisher_by_id),
                )
                .route("/get-event-publisher-list", get(publishers::get_event_publisher_list))
                .with_state(state),
        )
        // Merged after the trace layer so SSE payloads and secret webhook headers are not logged,
        // while still rejecting NUL bytes in both URI and request body.
        .layer(axum::middleware::from_fn(crate::reject_null_byte_uri))
}

/// Stream integration events as CloudEvents via SSE.
///
/// Streams CloudEvents v1.0 carrying Published Language integration events (`com.impierce.unicore.*`)
/// and caller provenance extensions (`callerid`, `callertype`).
///
/// # Note on delivery guarantee
///
/// This is a **live feed, not a source of truth.** Catch-up is bounded, and a reconnecting subscriber could
/// miss events. Gaps are signalled where detectable (`lagged`, `truncated`), not prevented.
///
/// This endpoint is suitable for a UI activity feed and external subscribers. Projections, syncs and audit consumers must reconcile against
/// the REST endpoints. Full detail in `problem-details/events`.
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
        ("limit" = Option<usize>, Query, description = "Optional limit on historical events; defaults to 100, clamped to 1000, set to 0 for live-only stream"),
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

    let limit = Some(params.limit.unwrap_or(DEFAULT_CATCHUP_LIMIT).min(MAX_CATCHUP_LIMIT));

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

    // 4a. Catch-up stopped on a limit, so events between the last one above and this
    // subscription's start are in neither stream.
    if catchup_result.truncated {
        let mut frame = sse::Event::default().event("truncated").data(
            json!({
                "warning": "Catch-up truncated; events between the last catch-up event and the live stream were not delivered",
                "limit": limit,
                "resume_after": catchup_result.resume_after,
            })
            .to_string(),
        );

        // A browser `EventSource` takes its next `Last-Event-ID` from the frame's `id` and never
        // from the payload, so without this a reconnect repeats the query that truncated.
        if let Some(resume_after) = catchup_result.resume_after.as_deref() {
            frame = frame.id(resume_after);
        }

        catchup_items.push(Ok(frame));
    }

    let catchup_stream = futures::stream::iter(catchup_items);

    // 5. Seamlessly transition to live stream, deduplicating any events that arrived during catch-up.
    let live_stream = live_subscription
        .filter(move |result| {
            let is_duplicate = match result {
                Ok(cloud_event) => seen_ids.contains(&cloud_event.id),
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
            Err(err) => {
                // Driver errors carry hostnames, ports and auth detail.
                tracing::error!(error = %err, "Event bus error on live SSE stream");
                Ok(sse::Event::default()
                    .event("error")
                    .data(json!({ "error": "Event bus error" }).to_string()))
            }
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
        let cred_event = shared_kernel::event_bus::CloudEvent::new(
            "com.impierce.unicore.issuance.credential.issued",
            "/services/credential",
        )
        .with_id("credential:cred-1:1")
        .with_subject("cred-1")
        .with_data(serde_json::json!({"id": "cred-1"}));
        let other_event =
            shared_kernel::event_bus::CloudEvent::new("com.impierce.unicore.other.created", "/services/other")
                .with_id("other:other-1:1")
                .with_subject("other-1")
                .with_data(serde_json::json!({"id": "other-1"}));
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
        let event1 = shared_kernel::event_bus::CloudEvent::new(
            "com.impierce.unicore.issuance.credential.issued",
            "/services/credential",
        )
        .with_id("credential:cred-1:1")
        .with_subject("cred-1")
        .with_data(serde_json::json!({}));
        let event2 = shared_kernel::event_bus::CloudEvent::new(
            "com.impierce.unicore.issuance.credential.revoked",
            "/services/credential",
        )
        .with_id("credential:cred-1:2")
        .with_subject("cred-1")
        .with_data(serde_json::json!({}));
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

        let event = shared_kernel::event_bus::CloudEvent::new(
            "com.impierce.unicore.issuance.credential.issued",
            "/services/credential",
        )
        .with_id("credential:cred-1:1")
        .with_subject("cred-1")
        .with_time(now)
        .with_data(serde_json::json!({}));
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
        let event1 = shared_kernel::event_bus::CloudEvent::new(
            "com.impierce.unicore.issuance.credential.issued",
            "/services/credential",
        )
        .with_id("credential:cred-1:1")
        .with_subject("cred-1")
        .with_data(serde_json::json!({}));
        bus_handle.publish(event1.clone());

        let app = router(Arc::new(bus_handle.clone().into()));
        let req = axum::http::Request::builder()
            .uri("/v0/events?sources=credential")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let event2 = shared_kernel::event_bus::CloudEvent::new(
            "com.impierce.unicore.issuance.credential.revoked",
            "/services/credential",
        )
        .with_id("credential:cred-1:2")
        .with_subject("cred-1")
        .with_data(serde_json::json!({}));
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
            .insert(shared_kernel::authorization::Actor::user("test-user"));
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

    /// Records the `limit` the handler passed down, so the clamp can be asserted via the route.
    struct RecordingReader {
        observed_limit: Arc<std::sync::Mutex<Option<Option<usize>>>>,
        truncated: bool,
        resume_after: Option<String>,
    }

    #[async_trait::async_trait]
    impl shared_kernel::event_bus::EventHistoryReader for RecordingReader {
        async fn history_ascending(
            &self,
            _filter: &EventFilter,
            _last_event_id: Option<&str>,
            limit: Option<usize>,
        ) -> Result<shared_kernel::event_bus::HistoryAscendingResult, EventBusError> {
            *self.observed_limit.lock().unwrap() = Some(limit);
            Ok(shared_kernel::event_bus::HistoryAscendingResult {
                events: Vec::new(),
                gap_detected: false,
                truncated: self.truncated,
                resume_after: self.resume_after.clone(),
            })
        }
    }

    async fn observe_limit(query: &str) -> Option<usize> {
        let observed_limit = Arc::new(std::sync::Mutex::new(None));
        let bus_handle = EventBusHandle::new(16);
        bus_handle.set_history_reader(Arc::new(RecordingReader {
            observed_limit: observed_limit.clone(),
            truncated: false,
            resume_after: None,
        }));

        let app = router(Arc::new(bus_handle.into()));
        let req = axum::http::Request::builder()
            .uri(format!("/v0/events{query}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let observed = *observed_limit.lock().unwrap();
        observed.expect("history reader was not called")
    }

    #[tokio::test]
    async fn test_events_sse_clamps_limit() {
        assert_eq!(observe_limit("").await, Some(DEFAULT_CATCHUP_LIMIT));
        assert_eq!(observe_limit("?limit=5").await, Some(5));
        assert_eq!(observe_limit("?limit=0").await, Some(0));
        assert_eq!(observe_limit("?limit=4294967295").await, Some(MAX_CATCHUP_LIMIT));
    }

    #[tokio::test]
    async fn test_events_sse_emits_truncated_frame() {
        let bus_handle = EventBusHandle::new(16);
        bus_handle.set_history_reader(Arc::new(RecordingReader {
            observed_limit: Arc::new(std::sync::Mutex::new(None)),
            truncated: true,
            resume_after: Some("credential:cred-9:42".to_string()),
        }));

        let app = router(Arc::new(bus_handle.into()));
        let req = axum::http::Request::builder()
            .uri("/v0/events")
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

        assert!(body_str.contains("event: truncated"), "unexpected frame: {body_str}");
        assert!(
            body_str.contains("id: credential:cred-9:42"),
            "truncated frame carries no resume cursor: {body_str}"
        );
        assert!(body_str.contains("\"resume_after\":\"credential:cred-9:42\""));
    }

    #[tokio::test]
    async fn test_events_sse_history_reader_error_does_not_leak_driver_detail() {
        struct LeakyReader;
        #[async_trait::async_trait]
        impl shared_kernel::event_bus::EventHistoryReader for LeakyReader {
            async fn history_ascending(
                &self,
                _filter: &EventFilter,
                _last_event_id: Option<&str>,
                _limit: Option<usize>,
            ) -> Result<shared_kernel::event_bus::HistoryAscendingResult, EventBusError> {
                Err(EventBusError::Source(
                    "server selection error: mongodb-0.internal:27017 auth failed".to_string(),
                ))
            }
        }

        let bus_handle = EventBusHandle::new(16);
        bus_handle.set_history_reader(Arc::new(LeakyReader));

        let app = router(Arc::new(bus_handle.into()));
        let req = axum::http::Request::builder()
            .uri("/v0/events")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body_str.contains("mongodb-0.internal"), "leaked host: {body_str}");
        assert!(!body_str.contains("27017"), "leaked port: {body_str}");
        assert!(!body_str.contains("auth failed"), "leaked auth detail: {body_str}");
    }

    #[tokio::test]
    async fn test_events_sse_rejects_null_byte_in_query() {
        let bus_handle = EventBusHandle::new(16);
        let app = router(Arc::new(bus_handle.into()));

        let req = axum::http::Request::builder()
            .uri("/v0/events?subject=%00")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = tower::ServiceExt::oneshot(app.clone(), req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

        let req = axum::http::Request::builder()
            .uri("/v0/events?subject=abc")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_event_publisher_http_routes_lifecycle_and_validation() {
        use bc_event_publishing::{
            application::{builder::EventPublishingContextBuilder, views::EventPublisherViewRepositories},
            domain::event_publisher::services::KnownEventTypes,
        };
        use shared_kernel::test_utils::in_memory::InMemoryStore;

        let bus_handle = EventBusHandle::new(64);
        let views = EventPublisherViewRepositories::from_factory(&InMemoryStore);
        let known_events = KnownEventTypes::with_event_types([
            "com.impierce.unicore.issuance.credential.offered",
            "com.impierce.unicore.issuance.credential.issued",
        ]);
        let context = EventPublishingContextBuilder::new()
            .with_views(views)
            .with_event_bus(bus_handle.clone())
            .with_known_event_types(known_events)
            .build(&InMemoryStore)
            .await
            .unwrap();

        let state = Arc::new(EventsState::from(bus_handle).with_event_publishing(Arc::new(context)));
        let app = router(state);

        // 1. NUL byte in request body is rejected with 400
        let nul_req = axum::http::Request::builder()
            .method("POST")
            .uri("/v0/create-event-publisher")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(r#"{"name":"bad\u0000"}"#))
            .unwrap();
        let nul_resp = tower::ServiceExt::oneshot(app.clone(), nul_req).await.unwrap();
        assert_eq!(nul_resp.status(), axum::http::StatusCode::BAD_REQUEST);

        // 2. Invalid targetUrl is rejected with 400
        let invalid_req = axum::http::Request::builder()
            .method("POST")
            .uri("/v0/create-event-publisher")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "name": "Bad Webhook",
                    "events": ["com.impierce.unicore.issuance.credential.issued"],
                    "config": {
                        "publisherType": "http",
                        "targetUrl": "ftp://invalid.example.org"
                    }
                }))
                .unwrap(),
            ))
            .unwrap();
        let invalid_resp = tower::ServiceExt::oneshot(app.clone(), invalid_req).await.unwrap();
        assert_eq!(invalid_resp.status(), axum::http::StatusCode::BAD_REQUEST);

        // 3. Create valid HTTP publisher -> 200 OK with redacted secret header
        let create_req = axum::http::Request::builder()
            .method("POST")
            .uri("/v0/create-event-publisher")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "name": "Live Webhook",
                    "description": "Initial",
                    "events": ["com.impierce.unicore.issuance.credential.issued"],
                    "config": {
                        "publisherType": "http",
                        "targetUrl": "https://subscriber.example.org/events",
                        "headers": {
                            "Authorization": "Bearer secret-123"
                        }
                    }
                }))
                .unwrap(),
            ))
            .unwrap();
        let create_resp = tower::ServiceExt::oneshot(app.clone(), create_req).await.unwrap();
        assert_eq!(create_resp.status(), axum::http::StatusCode::OK);
        let created_bytes = axum::body::to_bytes(create_resp.into_body(), usize::MAX).await.unwrap();
        let created_json: serde_json::Value = serde_json::from_slice(&created_bytes).unwrap();
        let pub_id = created_json["id"].as_str().unwrap().to_string();
        assert_eq!(created_json["enabled"], true);
        assert_eq!(created_json["publisherType"], "http");
        assert_eq!(created_json["config"]["headers"]["authorization"], "****");

        // 4. Get by ID and List
        let get_req = axum::http::Request::builder()
            .method("GET")
            .uri(format!("/v0/get-event-publisher-by-id/{pub_id}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let get_resp = tower::ServiceExt::oneshot(app.clone(), get_req).await.unwrap();
        assert_eq!(get_resp.status(), axum::http::StatusCode::OK);

        let list_req = axum::http::Request::builder()
            .method("GET")
            .uri("/v0/get-event-publisher-list")
            .body(axum::body::Body::empty())
            .unwrap();
        let list_resp = tower::ServiceExt::oneshot(app.clone(), list_req).await.unwrap();
        assert_eq!(list_resp.status(), axum::http::StatusCode::OK);
        let list_bytes = axum::body::to_bytes(list_resp.into_body(), usize::MAX).await.unwrap();
        let list_json: serde_json::Value = serde_json::from_slice(&list_bytes).unwrap();
        assert_eq!(list_json.as_array().unwrap().len(), 1);

        // 5. Update, Deactivate, Activate, Delete
        let update_req = axum::http::Request::builder()
            .method("POST")
            .uri("/v0/update-event-publisher")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "id": pub_id,
                    "name": "Updated Webhook",
                    "description": "Updated",
                    "events": ["com.impierce.unicore.issuance.*"],
                    "config": {
                        "publisherType": "http",
                        "targetUrl": "https://subscriber.example.org/v2/events"
                    }
                }))
                .unwrap(),
            ))
            .unwrap();
        let update_resp = tower::ServiceExt::oneshot(app.clone(), update_req).await.unwrap();
        assert_eq!(update_resp.status(), axum::http::StatusCode::OK);

        for endpoint in [
            "/v0/deactivate-event-publisher",
            "/v0/activate-event-publisher",
            "/v0/delete-event-publisher",
        ] {
            let req = axum::http::Request::builder()
                .method("POST")
                .uri(endpoint)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&json!({ "id": pub_id })).unwrap(),
                ))
                .unwrap();
            let resp = tower::ServiceExt::oneshot(app.clone(), req).await.unwrap();
            assert_eq!(resp.status(), axum::http::StatusCode::OK, "failed on {endpoint}");
        }

        // 6. Get after delete returns 404
        let get_deleted_req = axum::http::Request::builder()
            .method("GET")
            .uri(format!("/v0/get-event-publisher-by-id/{pub_id}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let get_deleted_resp = tower::ServiceExt::oneshot(app, get_deleted_req).await.unwrap();
        assert_eq!(get_deleted_resp.status(), axum::http::StatusCode::NOT_FOUND);
    }
}
