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
    config_mut().feature.kms = true;

    let state = application_state().await;
    state.verify_persisted_events().await;
    let issuance = state.api.issuance_state.as_ref().expect("issuance is enabled");
    agent_shared::handlers::public_command_handler(
        "persisted-reissuance",
        &issuance.command.reissuance,
        agent_issuance::reissuance::command::ReissuanceCommand::CreateReissuance {
            reissuance_id: "persisted-reissuance".to_owned(),
            original_credential_id: "original".to_owned(),
            new_credential_id: "replacement".to_owned(),
            offer_id: "replacement-offer".to_owned(),
            credential_configuration_id: "configuration".to_owned(),
            reason: None,
            trigger_type: None,
            triggered_by: None,
        },
    )
    .await
    .unwrap();
    let kms = state.api.kms_state.as_ref().expect("the KMS is enabled");
    agent_shared::handlers::public_command_handler(
        "persisted-key",
        &kms.command.keys,
        agent_kms::key::command::KeyCommand::RecordCreatedKey {
            reference: serde_json::from_value(json!({
                "provider_type": "aws_kms_ambient",
                "provider_key_id": "arn:aws:kms:eu-central-1:123456789012:key/00000000-0000-0000-0000-000000000001",
                "algorithm": "EdDSA",
                "public_key": { "kty": "OKP", "crv": "Ed25519", "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPZpiMlrwIaaPcHURo" }
            }))
            .unwrap(),
        },
    )
    .await
    .unwrap();
    let app = router(state);
    assert_reissuance_views(&app).await;

    let (status, _) = send(&app, Request::get("/readyz").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app,
        Request::post("/v0/add-key-provider")
            .header("Content-Type", "application/json")
            .body(Body::from(
                json!({ "providerType": "aws_kms_ambient", "name": "Persisted keys" }).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

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
    assert_reissuance_views(&app).await;

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

    let (status, providers) = send(
        &app,
        Request::get("/v0/get-all-key-providers").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        providers,
        json!([{ "providerType": "aws_kms_ambient", "name": "Persisted keys" }])
    );

    let (status, keys) = send(&app, Request::get("/v0/get-all-keys").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(keys[0]["id"], "persisted-key");
}

async fn assert_reissuance_views(app: &Router) {
    let (status, relation) = send(
        app,
        Request::get("/v0/get-credential-reissuance/persisted-reissuance")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(relation["new_credential_id"], "replacement");
    let (status, relations) = send(
        app,
        Request::get("/v0/list-all-credential-reissuances")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(relations
        .as_array()
        .unwrap()
        .iter()
        .any(|relation| relation["id"] == "persisted-reissuance"));
}
