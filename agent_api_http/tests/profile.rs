use agent_api_http::v0::identity::router;
use agent_identity::services::IdentityServices;
use agent_identity::{
    profile::{aggregate::Source, command::ProfileCommand},
    state::{IdentityState, PROFILE_ID},
};
use agent_shared::{
    config::{config, config_mut, ApplicationConfiguration},
    handlers::command_handler as internal_command_handler,
};
use agent_store::{identity_state, in_memory::InMemory};
use axum::{
    body::{to_bytes, Body},
    extract::Request,
    Router,
};
use http::StatusCode;
use serde_json::{json, Value};
use serial_test::serial;
use shared_kernel::authorization::Caller;
use std::sync::Arc;
use tower::ServiceExt;

struct ConfigSnapshot(ApplicationConfiguration);

impl ConfigSnapshot {
    fn capture() -> Self {
        Self(config().clone())
    }
}

impl Drop for ConfigSnapshot {
    fn drop(&mut self) {
        *config_mut() = self.0.clone();
    }
}

async fn setup() -> (Arc<IdentityState>, Router) {
    let state = Arc::new(
        identity_state(
            &InMemory,
            IdentityServices::default(),
            &shared_kernel::EventBusHandle::default(),
            vec![],
        )
        .await,
    );
    let app = router(state.clone());

    (state, app)
}

/// Creates a profile that mirrors the configured display, so that `query_profile` leaves the global configuration
/// unchanged for the other tests in this binary.
async fn create_profile(state: &IdentityState, source: Source) {
    let display = config().display.first().cloned().unwrap();

    internal_command_handler(
        state.authorization_checker.clone(),
        Caller::Internal,
        PROFILE_ID,
        &state.command.profile,
        ProfileCommand::CreateProfile {
            profile_id: PROFILE_ID.to_string(),
            display_name: Some(display.name),
            description: display.description,
            logo: display.logo,
            country: None,
            source,
        },
    )
    .await
    .unwrap();
}

async fn get(app: &Router) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::get("/v0/profile").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn patch(app: &Router, body: Value) -> StatusCode {
    app.clone()
        .oneshot(
            Request::patch("/v0/profile")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
#[serial]
async fn get_profile_returns_not_found_without_a_profile() {
    let _config = ConfigSnapshot::capture();
    let (_state, app) = setup().await;

    let (status, _) = get(&app).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn patch_profile_updates_every_given_field() {
    let _config = ConfigSnapshot::capture();
    let (state, app) = setup().await;
    create_profile(&state, Source::Default).await;
    let display = config().display.first().cloned().unwrap();

    let status = patch(
        &app,
        json!({
            "displayName": display.name,
            "description": null,
            "logo": display.logo,
            "country": "NL",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, profile) = get(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        profile,
        json!({
            "displayName": display.name,
            "logo": display.logo,
            "country": "NL",
            "source": "Runtime",
        })
    );
}

#[tokio::test]
#[serial]
async fn patch_profile_without_fields_leaves_the_profile_unchanged() {
    let _config = ConfigSnapshot::capture();
    let (state, app) = setup().await;
    create_profile(&state, Source::Default).await;

    assert_eq!(patch(&app, json!({})).await, StatusCode::OK);

    let (_, profile) = get(&app).await;
    assert_eq!(profile["source"], "Default");
}

#[tokio::test]
#[serial]
async fn patch_profile_is_rejected_for_a_provisioned_profile() {
    let _config = ConfigSnapshot::capture();
    let (state, app) = setup().await;
    create_profile(&state, Source::Provisioned).await;

    let status = patch(&app, json!({ "country": "NL" })).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (_, profile) = get(&app).await;
    assert_eq!(profile["source"], "Provisioned");
    assert!(profile.get("country").is_none());
}
