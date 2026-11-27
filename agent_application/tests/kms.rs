//! The Key Management System wired into the application: its keys sign for the `did:web` document.

use agent_application::{router, state};
use agent_secret_manager::{signer::SubjectSigner, subject::Subject};
use agent_shared::config::{config_mut, SupportedDidMethod, ToggleOptions};
use axum::{body::Body, extract::Request, Router};
use identity_kit::jose::{DefaultVerifier, Jwk, JwsAlgorithm, JwsVerifier, Signer};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

async fn post(app: &Router, uri: &str, body: Value) -> Value {
    let request = Request::post(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(
        status.is_success(),
        "{uri}: {status} {}",
        String::from_utf8_lossy(&body)
    );
    serde_json::from_slice(&body).unwrap_or(Value::Null)
}

#[tokio::test]
async fn a_kms_key_signs_with_the_key_its_provider_holds() {
    // Each file in `tests/` runs in its own process, so this only configures this test binary. The in-memory provider
    // and event store need the `development` profile.
    std::env::set_var("UNICORE__PROFILE", "development");
    std::env::set_var("UNICORE__EVENT_STORE__TYPE", "in_memory");
    config_mut().feature.kms = true;
    config_mut().did_methods = [(
        SupportedDidMethod::Web,
        ToggleOptions {
            enabled: true,
            preferred: Some(true),
        },
    )]
    .into();
    let subject = Arc::new(Subject::test_subject().await);
    let app = router(state(subject.clone()).await.unwrap());
    post(
        &app,
        "/v0/add-key-provider",
        json!({ "providerType": "in_memory", "name": "Local keys" }),
    )
    .await;
    let key = post(
        &app,
        "/v0/create-key",
        json!({ "providerType": "in_memory", "algorithm": "ES256" }),
    )
    .await;
    let method = post(&app, "/v0/add-key-to-document", json!({ "keyId": key["id"] })).await;
    post(
        &app,
        "/v0/select-signing-key",
        json!({ "verificationMethodId": method["verificationMethodId"] }),
    )
    .await;
    let signer = SubjectSigner::new(
        subject,
        SupportedDidMethod::Web.to_string(),
        jsonwebtoken::Algorithm::ES256,
    )
    .await
    .unwrap();

    let signature = signer.sign(b"message").await.unwrap();

    let public_key: Jwk = serde_json::from_value(key["publicKey"].clone()).unwrap();
    assert!(DefaultVerifier
        .verify(JwsAlgorithm::ES256, b"message", &signature, &public_key)
        .is_ok());
}
