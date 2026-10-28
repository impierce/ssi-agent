//! Endpoints of the Key Management System. Only mounted when the `kms` feature flag is enabled.

pub mod error;
pub mod key_providers;
pub mod openapi;

use crate::API_VERSION;
use agent_kms::KmsState;
use axum::{
    routing::{get, post},
    Router,
};
use key_providers::{
    add_key_provider, get_all_key_providers, get_key_provider_keys, remove_key_provider, test_key_provider_connection,
};
use std::sync::Arc;

pub fn router(kms_state: Arc<KmsState>) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/add-key-provider", post(add_key_provider))
                .route("/get-all-key-providers", get(get_all_key_providers))
                .route("/get-key-provider-keys/{provider_type}", get(get_key_provider_keys))
                .route("/remove-key-provider", post(remove_key_provider))
                .route("/test-key-provider-connection", post(test_key_provider_connection)),
        )
        .with_state(kms_state)
}

#[cfg(test)]
mod tests {
    use crate::{app_with_base_path, ApiState};
    use agent_kms::adapter::InMemoryKeyStore;
    use agent_kms::adapter::{test_utils::UnreachableKeyProvider, KeyProviderAdapter, KeyProviderAdapters};
    use agent_kms::key_provider::aggregate::KeyProviderType;
    use agent_kms::kms_state;
    use agent_store::in_memory::InMemory;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use shared_kernel::authorization::NoActorExtractor;
    use shared_kernel::event_bus::EventBusHandle;
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn app_with_kms() -> Router {
        let adapters: KeyProviderAdapters = [
            (
                KeyProviderType::InMemory,
                Arc::new(InMemoryKeyStore::new()) as Arc<dyn KeyProviderAdapter>,
            ),
            (KeyProviderType::AwsKmsAmbient, Arc::new(UnreachableKeyProvider)),
        ]
        .into();
        let kms_state = kms_state(&InMemory, &EventBusHandle::default(), true, adapters).await;

        app_with_base_path(
            ApiState {
                kms_state: Some(Arc::new(kms_state)),
                ..Default::default()
            },
            Arc::new(NoActorExtractor),
            "/",
        )
    }

    async fn send(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, body)
    }

    async fn add(app: &Router, provider_type: &str) {
        let body = json!({ "providerType": provider_type, "name": provider_type });
        assert_eq!(
            send(app, "POST", "/v0/add-key-provider", Some(body)).await.0,
            StatusCode::CREATED
        );
    }

    #[tokio::test]
    async fn kms_routes_do_not_exist_when_the_feature_is_disabled() {
        let app = app_with_base_path(ApiState::default(), Arc::new(NoActorExtractor), "/");

        for (method, uri) in [
            ("GET", "/v0/get-all-key-providers"),
            ("POST", "/v0/add-key-provider"),
            ("POST", "/v0/remove-key-provider"),
            ("POST", "/v0/test-key-provider-connection"),
            ("GET", "/v0/get-key-provider-keys/aws_kms_ambient"),
        ] {
            assert_eq!(send(&app, method, uri, None).await.0, StatusCode::NOT_FOUND, "{uri}");
        }
    }

    #[tokio::test]
    async fn adding_a_provider_answers_with_the_trimmed_name() {
        let app = app_with_kms().await;

        let body = json!({ "providerType": "in_memory", "name": "  Local keys  " });
        let response = send(&app, "POST", "/v0/add-key-provider", Some(body)).await;

        assert_eq!(
            response,
            (
                StatusCode::CREATED,
                json!({ "provider_type": "in_memory", "name": "Local keys" })
            )
        );
    }

    #[tokio::test]
    async fn lists_added_providers() {
        let app = app_with_kms().await;
        add(&app, "in_memory").await;

        let response = send(&app, "GET", "/v0/get-all-key-providers", None).await;

        assert_eq!(
            response,
            (
                StatusCode::OK,
                json!([{ "provider_type": "in_memory", "name": "in_memory" }])
            )
        );
    }

    #[tokio::test]
    async fn a_reachable_provider_is_reported_without_a_message() {
        let app = app_with_kms().await;
        add(&app, "in_memory").await;

        let body = json!({ "providerType": "in_memory" });
        let response = send(&app, "POST", "/v0/test-key-provider-connection", Some(body)).await;

        assert_eq!(response, (StatusCode::OK, json!({ "reachable": true })));
    }

    #[tokio::test]
    async fn an_unreachable_provider_is_reported_with_the_reason() {
        let app = app_with_kms().await;
        add(&app, "aws_kms_ambient").await;

        let body = json!({ "providerType": "aws_kms_ambient" });
        let response = send(&app, "POST", "/v0/test-key-provider-connection", Some(body)).await;

        assert_eq!(
            response,
            (
                StatusCode::OK,
                json!({ "reachable": false, "message": "connection refused" })
            )
        );
    }

    #[tokio::test]
    async fn listing_the_keys_of_an_unreachable_provider_is_a_bad_gateway() {
        let app = app_with_kms().await;
        add(&app, "aws_kms_ambient").await;

        let (status, body) = send(&app, "GET", "/v0/get-key-provider-keys/aws_kms_ambient", None).await;

        assert_eq!(
            (status, &body["detail"]),
            (StatusCode::BAD_GATEWAY, &json!("connection refused"))
        );
    }
}
