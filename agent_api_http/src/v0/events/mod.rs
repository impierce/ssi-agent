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
    pub until: Option<DateTime<Utc>>,
}

pub fn router(event_bus: EventBusHandle) -> Router {
    Router::new().nest(
        crate::API_VERSION,
        Router::new()
            .route("/events", get(events_sse_handler))
            .with_state(event_bus),
    )
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
        (status = 200, description = "Server-Sent Events stream of CloudEvents", body = shared_kernel::event_bus::CloudEvent, content_type = "text/event-stream")
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
        .map(|s| {
            s.split(',')
                .map(|item| item.trim().to_string())
                .filter(|i| !i.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let event_types: Vec<String> = params
        .types
        .map(|s| {
            s.split(',')
                .map(|item| item.trim().to_string())
                .filter(|i| !i.is_empty())
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
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let limit = params.limit.unwrap_or(50).min(500);

    // 1. Subscribe to live events FIRST to avoid missing published events in a race condition.
    let live_subscription = event_bus.subscribe(filter.clone());

    // 2. Query historical catch-up events.
    let catchup_result = event_bus.history_ascending(&filter, last_event_id.as_deref(), limit);

    let catchup_events = catchup_result.events;
    let mut seen_ids: std::collections::HashSet<String> = catchup_events.iter().map(|e| e.id.clone()).collect();

    let mut catchup_items = Vec::new();
    if catchup_result.gap_detected {
        catchup_items.push(Ok(sse::Event::default()
            .event("lagged")
            .data(json!({ "warning": "Last-Event-ID evicted from history" }).to_string())));
    }
    for cloud_event in catchup_events {
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

    let live_stream = live_subscription
        .filter(move |result| {
            let is_duplicate = match result {
                Ok(cloud_event) => !seen_ids.insert(cloud_event.id.clone()),
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

        let app = router(bus_handle.clone());

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
        .and_then(|f| f.ok())
        .and_then(|f| f.into_data().ok());

        let body_str = frame
            .map(|b| String::from_utf8(b.to_vec()).unwrap())
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

        let app = router(bus_handle.clone());

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
        .and_then(|f| f.ok())
        .and_then(|f| f.into_data().ok());

        let body_str = frame
            .map(|b| String::from_utf8(b.to_vec()).unwrap())
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

        let app = router(bus_handle.clone());

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
        .and_then(|f| f.ok())
        .and_then(|f| f.into_data().ok());

        let body_str = frame
            .map(|b| String::from_utf8(b.to_vec()).unwrap())
            .unwrap_or_default();

        assert!(body_str.contains(&event.id));
    }
}
