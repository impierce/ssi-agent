use agent_api_http::v0::holder::router;
use agent_holder::{holder_state, services::HolderServices};
use agent_secret_manager::{service::Service as _, subject::Subject};
use agent_store::in_memory::InMemory;
use axum::{
    body::{to_bytes, Body},
    extract::Request,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use http::StatusCode;
use serde_json::{json, Value};
use serial_test::serial;
use std::sync::Arc;
use tokio::sync::OnceCell;
use tower::ServiceExt;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

const CREDENTIAL_CONFIGURATION_ID: &str = "badge";

static SUBJECT: OnceCell<Arc<Subject>> = OnceCell::const_new();

async fn test_subject() -> Arc<Subject> {
    SUBJECT
        .get_or_init(|| async { Arc::new(Subject::test_subject().await) })
        .await
        .clone()
}

async fn setup() -> Router {
    let state = holder_state(
        &InMemory,
        Arc::new(HolderServices::new(test_subject().await)),
        &Default::default(),
    )
    .await;

    router(Arc::new(state))
}

/// An unsigned JWT: the holder only decodes a credential's claims when storing it.
fn credential_jwt(name: &str) -> String {
    let encode = |value: Value| URL_SAFE_NO_PAD.encode(value.to_string());

    format!(
        "{}.{}.signature",
        encode(json!({ "alg": "EdDSA", "typ": "JWT" })),
        encode(json!({
            "vc": {
                "@context": ["https://www.w3.org/2018/credentials/v1"],
                "type": ["VerifiableCredential"],
                "issuer": "did:example:issuer",
                "issuanceDate": "2010-01-01T00:00:00Z",
                "credentialSubject": { "name": name },
            }
        }))
    )
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
    send(app, Request::get(uri).body(Body::empty()).unwrap()).await
}

async fn post(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    send(
        app,
        Request::post(uri)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
}

async fn mock_issuer() -> MockServer {
    let mock_server = MockServer::start().await;
    let uri = mock_server.uri();

    Mock::given(method("GET"))
        .and(path("/.well-known/openid-credential-issuer"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "credential_issuer": uri,
            "credential_endpoint": format!("{uri}/credential"),
            "credential_configurations_supported": {
                CREDENTIAL_CONFIGURATION_ID: {
                    "format": "jwt_vc_json",
                    "credential_definition": { "type": ["VerifiableCredential"] }
                }
            }
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-authorization-server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issuer": uri,
            "token_endpoint": format!("{uri}/token"),
            "pre-authorized_grant_anonymous_access_supported": true
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access-token",
            "token_type": "bearer"
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/credential"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "credentials": [{ "credential": credential_jwt("Offered") }]
        })))
        .mount(&mock_server)
        .await;

    mock_server
}

/// Receives a credential offer from `credential_issuer` and returns the ID of the received offer.
async fn receive_offer(app: &Router, credential_issuer: &str) -> String {
    let credential_offer = json!({
        "credential_issuer": credential_issuer,
        "credential_configuration_ids": [CREDENTIAL_CONFIGURATION_ID],
        "grants": {
            "urn:ietf:params:oauth:grant-type:pre-authorized_code": { "pre-authorized_code": "code" }
        }
    });
    let query = serde_urlencoded::to_string([("credential_offer", credential_offer.to_string())]).unwrap();

    let (status, _) = get(app, &format!("/credential_offer?{query}")).await;
    assert_eq!(status, StatusCode::OK);

    let (_, offers) = get(app, "/v0/holder/offers").await;
    offers.as_array().unwrap().last().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
#[serial]
async fn credentials_can_be_stored_and_presented() {
    let app = setup().await;

    let (status, credential) = post(
        &app,
        "/v0/holder/credentials",
        json!({ "credential": credential_jwt("Ferris") }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let credential_id = credential["id"].as_str().unwrap().to_string();

    let (status, credentials) = get(&app, "/v0/holder/credentials").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(credentials.as_array().unwrap().len(), 1);

    let (status, credential) = get(&app, &format!("/v0/holder/credentials/{credential_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(credential["data"]["raw"]["credentialSubject"]["name"], "Ferris");

    let (status, presentation) = post(
        &app,
        "/v0/holder/presentations",
        json!({ "credentialIds": [credential_id] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let presentation_id = presentation["id"].as_str().unwrap().to_string();

    let (status, presentations) = get(&app, "/v0/holder/presentations").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(presentations.as_array().unwrap().len(), 1);

    let (status, presentation) = get(&app, &format!("/v0/holder/presentations/{presentation_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(presentation["id"], presentation_id.as_str());
    assert!(presentation["signed"].is_string());

    let (status, _) = get(&app, &format!("/v0/holder/presentations/{presentation_id}/signed")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn unknown_holder_resources_return_not_found() {
    let app = setup().await;

    for uri in [
        "/v0/holder/credentials/unknown",
        "/v0/holder/presentations/unknown",
        "/v0/holder/offers/unknown",
    ] {
        let (status, _) = get(&app, uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }

    let (status, _) = post(
        &app,
        "/v0/holder/presentations",
        json!({ "credentialIds": ["unknown"] }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    for uri in ["/v0/holder/offers/unknown/accept", "/v0/holder/offers/unknown/reject"] {
        let (status, _) = post(&app, uri, json!({})).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
#[serial]
async fn an_undecodable_credential_is_rejected() {
    let app = setup().await;

    let (status, _) = post(&app, "/v0/holder/credentials", json!({ "credential": "not-a-jwt" })).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
#[serial]
async fn empty_collections_are_listed_as_empty_arrays() {
    let app = setup().await;

    for uri in [
        "/v0/holder/credentials",
        "/v0/holder/presentations",
        "/v0/holder/offers",
    ] {
        let (status, body) = get(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert_eq!(body, json!([]), "{uri}");
    }
}

#[tokio::test]
#[serial]
async fn invalid_credential_offers_are_rejected() {
    let app = setup().await;

    let (status, _) = get(&app, "/credential_offer").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = get(&app, "/credential_offer?credential_offer=not-json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn a_credential_offer_from_an_unreachable_issuer_is_not_stored() {
    let app = setup().await;
    let mock_server = MockServer::start().await;

    let credential_offer = json!({
        "credential_issuer": mock_server.uri(),
        "credential_configuration_ids": [CREDENTIAL_CONFIGURATION_ID],
    });
    let query = serde_urlencoded::to_string([("credential_offer", credential_offer.to_string())]).unwrap();

    let (status, _) = get(&app, &format!("/credential_offer?{query}")).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);

    let (_, offers) = get(&app, "/v0/holder/offers").await;
    assert_eq!(offers, json!([]));
}

#[tokio::test]
#[serial]
async fn a_received_offer_can_be_rejected_once() {
    let app = setup().await;
    let mock_server = mock_issuer().await;
    let offer_id = receive_offer(&app, &mock_server.uri()).await;

    let (status, offer) = get(&app, &format!("/v0/holder/offers/{offer_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(offer["status"], "Pending");

    let reject_uri = format!("/v0/holder/offers/{offer_id}/reject");
    let (status, _) = post(&app, &reject_uri, json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, offer) = get(&app, &format!("/v0/holder/offers/{offer_id}")).await;
    assert_eq!(offer["status"], "Rejected");

    let (status, _) = post(&app, &reject_uri, json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
#[serial]
async fn accepting_an_offer_stores_the_issued_credential() {
    let app = setup().await;
    let mock_server = mock_issuer().await;
    let offer_id = receive_offer(&app, &mock_server.uri()).await;

    let (status, offer) = post(&app, &format!("/v0/holder/offers/{offer_id}/accept"), json!({})).await;
    assert_eq!(status, StatusCode::CREATED, "{offer}");
    assert_eq!(offer["status"], "CredentialsReceived");

    let (_, credentials) = get(&app, "/v0/holder/credentials").await;
    assert_eq!(credentials.as_array().unwrap().len(), 1);
    assert_eq!(credentials[0]["received_offer_id"], offer_id.as_str());
    assert_eq!(credentials[0]["data"]["raw"]["credentialSubject"]["name"], "Offered");
}
