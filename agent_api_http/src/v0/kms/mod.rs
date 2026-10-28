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
    use agent_kms::adapter::{ConnectionError, InMemoryKeyProvider, KeyProviderAdapter, KeyProviderAdapters};
    use agent_kms::key_provider::aggregate::KeyProviderType;
    use agent_kms::kms_state;
    use agent_store::in_memory::InMemory;
    use async_trait::async_trait;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use shared_kernel::authorization::NoActorExtractor;
    use shared_kernel::event_bus::EventBusHandle;
    use std::sync::Arc;
    use tower::ServiceExt;

    struct Unreachable;

    #[async_trait]
    impl KeyProviderAdapter for Unreachable {
        async fn test_connection(&self) -> Result<(), ConnectionError> {
            Err(ConnectionError("no credentials".to_string()))
        }

        async fn list_keys(&self) -> Result<agent_kms::adapter::ProviderKeys, ConnectionError> {
            Err(ConnectionError("no credentials".to_string()))
        }
    }

    async fn app_with_kms() -> Router {
        let adapters: KeyProviderAdapters = [
            (
                KeyProviderType::InMemory,
                Arc::new(InMemoryKeyProvider) as Arc<dyn KeyProviderAdapter>,
            ),
            (KeyProviderType::AwsKmsAmbient, Arc::new(Unreachable)),
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
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
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
    async fn providers_can_be_added_listed_tested_and_removed() {
        let app = app_with_kms().await;

        let (status, body) = send(
            &app,
            "POST",
            "/v0/add-key-provider",
            Some(json!({ "providerType": "in_memory", "name": "  Local keys  " })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body, json!({ "provider_type": "in_memory", "name": "Local keys" }));

        let (status, _) = send(
            &app,
            "POST",
            "/v0/add-key-provider",
            Some(json!({ "providerType": "aws_kms_ambient", "name": "Production keys" })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, body) = send(&app, "GET", "/v0/get-all-key-providers", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            json!([
                { "provider_type": "in_memory", "name": "Local keys" },
                { "provider_type": "aws_kms_ambient", "name": "Production keys" },
            ])
        );

        let (status, body) = send(
            &app,
            "POST",
            "/v0/test-key-provider-connection",
            Some(json!({ "providerType": "in_memory" })),
        )
        .await;
        assert_eq!((status, body), (StatusCode::OK, json!({ "reachable": true })));

        let (status, body) = send(
            &app,
            "POST",
            "/v0/test-key-provider-connection",
            Some(json!({ "providerType": "aws_kms_ambient" })),
        )
        .await;
        assert_eq!(
            (status, body),
            (
                StatusCode::OK,
                json!({ "reachable": false, "message": "no credentials" })
            )
        );

        let (status, _) = send(
            &app,
            "POST",
            "/v0/remove-key-provider",
            Some(json!({ "providerType": "in_memory" })),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (_, body) = send(&app, "GET", "/v0/get-all-key-providers", None).await;
        assert_eq!(
            body,
            json!([{ "provider_type": "aws_kms_ambient", "name": "Production keys" }])
        );
    }

    #[tokio::test]
    async fn adding_a_second_provider_of_a_type_conflicts() {
        let app = app_with_kms().await;
        let add = json!({ "providerType": "in_memory", "name": "Local keys" });

        assert_eq!(
            send(&app, "POST", "/v0/add-key-provider", Some(add.clone())).await.0,
            StatusCode::CREATED
        );
        assert_eq!(
            send(&app, "POST", "/v0/add-key-provider", Some(add)).await.0,
            StatusCode::CONFLICT
        );
    }

    #[tokio::test]
    async fn lists_the_keys_of_an_added_provider() {
        let app = app_with_kms().await;
        for provider_type in ["in_memory", "aws_kms_ambient"] {
            send(
                &app,
                "POST",
                "/v0/add-key-provider",
                Some(json!({ "providerType": provider_type, "name": provider_type })),
            )
            .await;
        }

        let (status, body) = send(&app, "GET", "/v0/get-key-provider-keys/in_memory", None).await;
        assert_eq!(
            (status, body),
            (StatusCode::OK, json!({ "keys": [], "hidden_keys": 0 }))
        );

        let (status, body) = send(&app, "GET", "/v0/get-key-provider-keys/aws_kms_ambient", None).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["detail"], json!("no credentials"));
    }

    #[tokio::test]
    async fn testing_a_provider_that_was_not_added_is_not_found() {
        let app = app_with_kms().await;

        let (status, _) = send(
            &app,
            "POST",
            "/v0/test-key-provider-connection",
            Some(json!({ "providerType": "aws_kms_ambient" })),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
