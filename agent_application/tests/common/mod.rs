//! Shared flow for the event store integration tests.

use agent_application::{router, state, ApplicationState};
use agent_secret_manager::subject::Subject;
use agent_shared::config::{config_mut, EventStoreConfig, EventStoreType};
use axum::{body::Body, extract::Request, http::StatusCode, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();

    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn application_state() -> ApplicationState {
    state(Arc::new(Subject::test_subject().await)).await.unwrap()
}

/// Starts the application against the given event store, writes through the API, then starts it again on the same
/// store (as after a restart), and asserts that the persisted events are verified and the data is served again.
///
/// The event store is configured through [`config_mut`], since the `test_utils` configuration overrides
/// `UNICORE__EVENT_STORE__CONNECTION_STRING`. Each file in `tests/` runs in its own process, so this only affects the
/// configuration of the calling test binary.
pub async fn assert_state_survives_a_restart(type_: EventStoreType, connection_string: String) {
    config_mut().event_store = EventStoreConfig {
        type_,
        connection_string: Some(connection_string),
    };

    let state = application_state().await;
    state.verify_persisted_events().await;
    let app = router(state);

    let (status, _) = send(&app, Request::get("/readyz").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);

    let (status, catalog) = send(
        &app,
        Request::post("/v0/create-new-catalog")
            .header("Content-Type", "application/json")
            .body(Body::from(
                json!({ "display": { "name": "Persisted Catalog", "description": "" } }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let catalog_id = catalog["id"].as_str().unwrap().to_string();

    // Restart: a new application state on the same event store.
    let state = application_state().await;
    let report = state.event_verification.verify_events().await.unwrap();
    assert!(report.is_compatible(), "{:?}", report.incompatible);
    assert!(report.checked > 0, "no persisted events were verified");

    state.verify_persisted_events().await;
    let app = router(state);

    let (status, _) = send(&app, Request::get("/readyz").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);

    let (status, catalog) = send(
        &app,
        Request::get(format!("/v0/get-catalog-by-id/{catalog_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(catalog["display"]["name"], "Persisted Catalog");

    let (status, profile) = send(&app, Request::get("/v0/profile").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(profile["source"], "Provisioned");
}
