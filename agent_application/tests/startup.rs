//! Smoke tests: the complete application state and router start up against the in-memory event store and serve
//! requests on every surface (probes, metadata, configuration, API and protocol endpoints).

use agent_application::{core_event_verifiers, router, state, ApplicationState};
use agent_secret_manager::subject::Subject;
use axum::{body::Body, extract::Request, http::StatusCode, Router};
use serde_json::Value;
use std::sync::{Arc, Once};
use tower::ServiceExt;

/// Each file in `tests/` runs in its own process, so this only affects the configuration of this test binary. The
/// `production` profile refuses the in-memory event store, since its events would be lost on restart.
fn use_in_memory_event_store() {
    static IN_MEMORY: Once = Once::new();
    IN_MEMORY.call_once(|| {
        std::env::set_var("UNICORE__PROFILE", "development");
        std::env::set_var("UNICORE__EVENT_STORE__TYPE", "in_memory");
    });
}

async fn application_state() -> ApplicationState {
    use_in_memory_event_store();
    state(Arc::new(Subject::test_subject().await)).await.unwrap()
}

async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();

    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn the_application_starts_and_serves_every_surface() {
    let state = application_state().await;
    state.verify_persisted_events().await;
    let app = router(state);

    for uri in [
        "/healthz",
        "/livez",
        "/readyz",
        "/version",
        "/info",
        "/v0/configuration",
        "/.well-known/openid-credential-issuer",
        "/.well-known/oauth-authorization-server",
    ] {
        let (status, _) = get(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
    }

    // Startup initializes the organisation profile from the configured display.
    let (status, profile) = get(&app, "/v0/profile").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(profile["source"], "Provisioned");

    let (_, info) = get(&app, "/info").await;
    assert_eq!(info["app"], "UniCore");
}

#[tokio::test]
async fn the_application_is_not_ready_until_its_persisted_events_are_verified() {
    let app = router(application_state().await);

    let (status, _) = get(&app, "/readyz").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    // Liveness does not depend on readiness.
    let (status, _) = get(&app, "/livez").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn readiness_can_be_granted_explicitly_or_by_custom_event_verifiers() {
    let state = application_state().await;
    state.verify_persisted_events_with(core_event_verifiers()).await;
    let (status, _) = get(&router(state), "/readyz").await;
    assert_eq!(status, StatusCode::OK);

    let state = application_state().await;
    state.mark_ready();
    let (status, _) = get(&router(state), "/readyz").await;
    assert_eq!(status, StatusCode::OK);
}
