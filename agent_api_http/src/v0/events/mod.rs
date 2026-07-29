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
use shared_kernel::event_bus::{EventBus, EventBusError, EventBusHandle, EventFilter};
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct EventQueryParams {
    pub types: Option<String>,
    pub sources: Option<String>,
    pub subject: Option<String>,
    pub limit: Option<usize>,
    pub since: Option<DateTime<Utc>>,
    pub after: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    // For backwards compatibility
    pub aggregate_types: Option<String>,
    pub event_types: Option<String>,
    pub aggregate_id: Option<String>,
}

pub fn router(event_bus: EventBusHandle) -> Router {
    Router::new()
        .route("/events", get(events_sse_handler))
        .with_state(event_bus)
}

/// Stream domain events as CloudEvents via SSE with Catch-Up.
#[utoipa::path(
    get,
    path = "/events",
    params(
        ("types" = Option<String>, Query, description = "Comma-separated list of CloudEvent types to filter"),
        ("sources" = Option<String>, Query, description = "Comma-separated list of sources/aggregate types to filter"),
        ("subject" = Option<String>, Query, description = "Optional aggregate/subject ID filter"),
        ("since" = Option<String>, Query, description = "Filter events after RFC 3339 timestamp"),
        ("until" = Option<String>, Query, description = "Filter events before RFC 3339 timestamp")
    ),
    responses(
        (status = 200, description = "Server-Sent Events stream of CloudEvents", content_type = "text/event-stream")
    ),
    tag = "Events"
)]
pub async fn events_sse_handler(
    State(event_bus): State<EventBusHandle>,
    headers: axum::http::HeaderMap,
    Query(params): Query<EventQueryParams>,
) -> Sse<impl Stream<Item = Result<sse::Event, axum::Error>>> {
    let sources: Vec<String> = params
        .sources
        .or(params.aggregate_types)
        .map(|s| {
            s.split(',')
                .map(|item| item.trim().to_string())
                .filter(|i| !i.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let event_types: Vec<String> = params
        .types
        .or(params.event_types)
        .map(|s| {
            s.split(',')
                .map(|item| item.trim().to_string())
                .filter(|i| !i.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let subject = params.subject.or(params.aggregate_id);
    let since = params.since.or(params.after);
    let until = params.until.or(params.before);

    let filter = EventFilter {
        event_types,
        sources,
        subject,
        since,
        until,
    };

    let last_event_id = headers
        .get("last-event-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let limit = params.limit.unwrap_or(50).min(500);

    let catchup_events = event_bus
        .history_ascending(&filter, last_event_id.as_deref(), limit)
        .await;

    let catchup_stream = futures::stream::iter(catchup_events.into_iter().map(|cloud_event| {
        let event_type = cloud_event.event_type.clone();
        let event_id = cloud_event.id.clone();
        match serde_json::to_string(&cloud_event) {
            Ok(json_data) => Ok(sse::Event::default().id(event_id).event(event_type).data(json_data)),
            Err(err) => Ok(sse::Event::default()
                .event("error")
                .data(format!("Serialization error: {}", err))),
        }
    }));

    let live_stream = event_bus.subscribe(filter).map(move |result| match result {
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
        Err(EventBusError::Lagged(n)) => Ok(sse::Event::default()
            .event("lagged")
            .data(json!({ "dropped": n }).to_string())),
        Err(err) => Ok(sse::Event::default()
            .event("error")
            .data(format!("Event bus error: {}", err))),
    });

    let sse_stream = catchup_stream.chain(live_stream);

    Sse::new(sse_stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_events_sse_route() {
        let bus_handle = EventBusHandle::new(16);
        let app = router(bus_handle.clone());

        let req = axum::http::Request::builder()
            .uri("/events?sources=credential")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(response.headers().get("content-type").unwrap(), "text/event-stream");
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
        bus_handle.publish(event2);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let app = router(bus_handle.clone());

        let req = axum::http::Request::builder()
            .uri("/events?sources=credential")
            .header("last-event-id", event1.id)
            .body(axum::body::Body::empty())
            .unwrap();

        let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
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
        bus_handle.publish(event);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let app = router(bus_handle.clone());

        let past_time = (now - chrono::Duration::hours(1)).to_rfc3339();
        let uri = format!("/events?sources=credential&since={}", urlencoding::encode(&past_time));

        let req = axum::http::Request::builder()
            .uri(&uri)
            .body(axum::body::Body::empty())
            .unwrap();

        let response = tower::ServiceExt::oneshot(app, req).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }
}
