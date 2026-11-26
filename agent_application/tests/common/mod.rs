//! Shared flow for the event store integration tests.

use agent_application::{router, state, ApplicationState};
use agent_identity::document::command::DocumentCommand;
use agent_kms::key::{aggregate::KeyReference, command::KeyCommand};
use agent_kms::key_provider::aggregate::KeyProviderType;
use agent_secret_manager::subject::Subject;
use agent_shared::config::{config_mut, EventStoreConfig, EventStoreType, SupportedDidMethod, ToggleOptions};
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
    config_mut().did_methods.insert(
        SupportedDidMethod::Web,
        ToggleOptions {
            enabled: true,
            preferred: Some(true),
        },
    );

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
    let kms = state.api.kms_state.clone().expect("the KMS is enabled");
    let identity = state.api.identity_state.clone().expect("identity is enabled");
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

    // Recorded directly: a key can't be created in AWS KMS here, and in-memory keys need the `development` profile.
    agent_shared::handlers::command_handler(
        kms.authorization_checker.clone(),
        shared_kernel::authorization::Caller::Internal,
        "persisted-key",
        &kms.command.keys,
        KeyCommand::CreateKey {
            reference: KeyReference {
                provider_type: KeyProviderType::AwsKmsAmbient,
                provider_key_id: "arn:aws:kms:eu-central-1:123456789012:key/persisted".into(),
                label: Some("issuer".to_string()),
                algorithm: identity_kit::jose::JwsAlgorithm::ES256,
                public_key: serde_json::from_value(json!({
                    "kty": "EC",
                    "crv": "P-256",
                    "x": "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
                    "y": "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0"
                }))
                .unwrap(),
            },
        },
    )
    .await
    .unwrap();

    // Added and selected directly, like the key: the AWS KMS provider can't be asked for it here.
    let (status, documents) = send(
        &app,
        Request::get("/v0/documents?did_method=did:web")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let document_id = documents[0]["id"].as_str().unwrap().to_string();
    let method = format!(
        "{}#oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U",
        documents[0]["document"]["id"].as_str().unwrap()
    );
    for command in [
        DocumentCommand::AddPublicKey {
            public_key: Box::new(
                serde_json::from_value(json!({
                    "kty": "EC",
                    "crv": "P-256",
                    "alg": "ES256",
                    "x": "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
                    "y": "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0"
                }))
                .unwrap(),
            ),
            key_id: "persisted-key".to_string(),
        },
        DocumentCommand::SelectSigningKey {
            verification_method_id: method.parse().unwrap(),
        },
    ] {
        agent_shared::handlers::public_command_handler(&document_id, &identity.command.document, command)
            .await
            .unwrap();
    }

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

    let (status, documents) = send(
        &app,
        Request::get("/v0/documents?did_method=did:web")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&documents[0]["added_keys"], &documents[0]["signing_methods"]),
        (&json!({ &method: "persisted-key" }), &json!({ "ES256": &method }))
    );

    // The key can't be loaded from AWS KMS here, so the configured key signs instead of the selected one.
    let (status, signing_keys) = send(&app, Request::get("/v0/get-signing-keys").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    let es256 = signing_keys
        .as_array()
        .unwrap()
        .iter()
        .find(|signing_key| signing_key["algorithm"] == "ES256")
        .unwrap();
    assert_eq!(es256["selectedVerificationMethodId"], json!(&method));
    assert_ne!(es256["verificationMethodId"], json!(&method));
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
