pub mod domain_linkage;
pub mod linked_vp;

use crate::extractors::RequestActor;
use crate::handlers::query_handler;
use agent_identity::state::IdentityState;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use std::sync::Arc;

#[axum_macros::debug_handler]
pub(crate) async fn services(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
) -> Result<Response, ApiError> {
    let all_services = query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        "all_services",
        &state.query.all_services,
    )
    .await?
    .map(|all_services_view| all_services_view.services.into_values().collect::<Vec<_>>())
    .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_services)).into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn service(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Path(service_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &service_id,
        &state.query.service,
    )
    .await?
    .map(|service_view| (StatusCode::OK, Json(service_view)).into_response())
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[cfg(test)]
mod tests {
    use agent_identity::{
        document::aggregate::Document,
        service::lifecycle::{maintain_services, spawn_maintenance},
        services::IdentityServices,
        state::{initialize_documents, IdentityState},
    };
    use agent_secret_manager::subject::{StorageKey, Subject};
    use agent_shared::config::{config, ApplicationConfiguration, SupportedDidMethod, ToggleOptions};
    use axum::{
        body::{to_bytes, Body},
        extract::Request,
        Router,
    };
    use identity_core::common::Timestamp;
    use identity_credential::{
        domain_linkage::{DomainLinkageConfiguration, JwtDomainLinkageValidator},
        validator::JwtCredentialValidationOptions,
    };
    use identity_document::document::CoreDocument;
    use oid4vc_core::verifier::SignatureVerifier;
    use serde_json::{json, Value};
    use shared_kernel::authorization::{
        Actor, ActorExtractor, AuthorizationChecker, AuthorizationError, AuthorizationOperation, AuthorizationRequest,
        ToActor,
    };
    use std::sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc, Mutex,
    };
    use tower::ServiceExt;
    use url::Url;

    #[derive(Default)]
    struct RecordingAuthorization {
        requests: Mutex<Vec<AuthorizationRequest>>,
        denied: AtomicBool,
    }

    #[async_trait::async_trait]
    impl AuthorizationChecker for RecordingAuthorization {
        async fn is_authorized(&self, request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            self.requests.lock().unwrap().push(request.clone());
            if request.actor.is_none() {
                return Err(AuthorizationError::Unauthorized);
            }
            if self.denied.load(Ordering::SeqCst) {
                return Err(AuthorizationError::Forbidden);
            }
            Ok(())
        }
    }

    struct HeaderActor;
    #[async_trait::async_trait]
    impl ActorExtractor for HeaderActor {
        async fn extract_actor(&self, input: &dyn ToActor) -> Option<Actor> {
            input.bearer_token().filter(|token| *token == "test").map(|_| Actor {
                subject: "administrator".into(),
            })
        }
    }

    struct Fixture {
        state: Arc<IdentityState>,
        app: Router,
        configuration: ApplicationConfiguration,
        clock: Arc<AtomicI64>,
        authorization: Arc<RecordingAuthorization>,
    }

    impl Fixture {
        async fn new() -> Self {
            Self::with_public_url("https://example.org/unicore/".parse().unwrap()).await
        }

        /// Like [`Fixture::new`], but the deployment identity and its published `.well-known`
        /// resources live at `public_url` instead of the fixed `https://example.org/unicore/`. Used
        /// to point domain linkage self-verification at a mock server it can actually reach.
        async fn with_public_url(public_url: Url) -> Self {
            let mut configuration = config().clone();
            configuration.application_url = "http://localhost:3033/unicore/".parse().unwrap();
            configuration.public_url = public_url;
            configuration.did_methods = [(
                SupportedDidMethod::Web,
                ToggleOptions {
                    enabled: true,
                    preferred: Some(true),
                },
            )]
            .into();
            let clock = Arc::new(AtomicI64::new(Timestamp::now_utc().to_unix()));
            let mut services = IdentityServices::new(Arc::new(Subject::test_subject().await));
            services.public_url = configuration.public_url.clone();
            services.linkage_clock = {
                let clock = clock.clone();
                Arc::new(move || Timestamp::from_unix(clock.load(Ordering::SeqCst)).unwrap())
            };
            let mut state =
                agent_store::identity_state(&agent_store::in_memory::InMemory, Arc::new(services), vec![]).await;
            initialize_documents(&state, &configuration).await.unwrap();
            let authorization = Arc::new(RecordingAuthorization::default());
            state.authorization_checker = authorization.clone();
            let state = Arc::new(state);
            let app = crate::app_with_base_path(
                crate::ApiState {
                    identity_state: Some(state.clone()),
                    ..Default::default()
                },
                Arc::new(HeaderActor),
                configuration.application_url.path(),
            );
            Self {
                state,
                app,
                configuration,
                clock,
                authorization,
            }
        }

        async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Vec<u8>) {
            let request = Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", "Bearer test")
                .header("content-type", "application/json")
                .body(
                    body.map(|body| Body::from(body.to_string()))
                        .unwrap_or_else(Body::empty),
                )
                .unwrap();
            let response = self.app.clone().oneshot(request).await.unwrap();
            (
                response.status().as_u16(),
                to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec(),
            )
        }

        async fn post(&self, command: &str, body: Option<Value>, expected: u16) {
            let (status, response) = self.request("POST", &format!("/unicore/v0/{command}"), body).await;
            assert_eq!(status, expected, "{}", String::from_utf8_lossy(&response));
        }

        async fn did(&self) -> Vec<u8> {
            let (status, body) = self.request("GET", "/.well-known/did.json", None).await;
            assert_eq!(status, 200);
            body
        }

        async fn linkage(&self) -> Vec<u8> {
            let (status, body) = self.request("GET", "/.well-known/did-configuration.json", None).await;
            assert_eq!(status, 200);
            body
        }

        async fn restart(&self) {
            self.state.services.subject.verification_method_ids.lock().await.clear();
            initialize_documents(&self.state, &self.configuration).await.unwrap();
            maintain_services(&self.state).await.unwrap();
        }

        async fn validate(&self) {
            let document: CoreDocument = serde_json::from_slice(&self.did().await).unwrap();
            let configuration: DomainLinkageConfiguration = serde_json::from_slice(&self.linkage().await).unwrap();
            let now = Timestamp::from_unix(self.clock.load(Ordering::SeqCst)).unwrap();
            let options = JwtCredentialValidationOptions::default()
                .latest_issuance_date(now)
                .earliest_expiry_date(now);
            JwtDomainLinkageValidator::with_signature_verifier(SignatureVerifier)
                .validate_linkage(
                    &document,
                    &configuration,
                    &"https://example.org".parse().unwrap(),
                    &options,
                )
                .unwrap();
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn runtime_services_preserve_identity_and_survive_restart() {
        let fixture = Fixture::new().await;
        let original = fixture.did().await;
        assert_eq!(fixture.request("GET", "/unicore/v0/documents", None).await.0, 200);
        assert_eq!(fixture.request("GET", "/v0/documents", None).await.0, 404);
        assert_eq!(
            fixture.request("GET", "/unicore/.well-known/did.json", None).await.0,
            404
        );
        assert_eq!(
            fixture
                .request("GET", "/.well-known/did-configuration.json", None)
                .await
                .0,
            404
        );
        fixture.post("remove-domain-linkage", None, 404).await;
        fixture.post("remove-linked-verifiable-presentation", None, 404).await;
        fixture
            .post("services/linked-vp", Some(json!({"presentationIds": []})), 405)
            .await;

        fixture.post("create-domain-linkage", None, 204).await;
        fixture.post("create-domain-linkage", None, 409).await;
        fixture.validate().await;
        fixture
            .post(
                "create-linked-verifiable-presentation",
                Some(json!({"presentationIds": ["presentation-1"]})),
                204,
            )
            .await;
        fixture
            .post(
                "create-linked-verifiable-presentation",
                Some(json!({"presentationIds": ["presentation-1"]})),
                409,
            )
            .await;
        let document = fixture.did().await;
        let value: Value = serde_json::from_slice(&document).unwrap();
        assert_eq!(value["id"], "did:web:example.org");
        assert!(String::from_utf8_lossy(&document)
            .contains("https://example.org/unicore/linked-verifiable-presentations/presentation-1"));
        let linkage = fixture.linkage().await;
        fixture.restart().await;
        assert_eq!(fixture.did().await, document);
        assert_eq!(fixture.linkage().await, linkage);
        assert!(fixture
            .state
            .services
            .subject
            .get_verification_method_id(StorageKey::new(SupportedDidMethod::Web, jsonwebtoken::Algorithm::EdDSA))
            .await
            .is_some());

        fixture.post("remove-domain-linkage", None, 204).await;
        assert_eq!(
            fixture
                .request("GET", "/.well-known/did-configuration.json", None)
                .await
                .0,
            404
        );
        let after_removal: Value = serde_json::from_slice(&fixture.did().await).unwrap();
        assert_eq!(after_removal["service"].as_array().unwrap().len(), 1);
        fixture.post("remove-linked-verifiable-presentation", None, 204).await;
        fixture.restart().await;
        assert_eq!(fixture.did().await, original);
        assert_eq!(
            fixture
                .request("GET", "/.well-known/did-configuration.json", None)
                .await
                .0,
            404
        );
        fixture.post("create-domain-linkage", None, 204).await;
        fixture
            .post(
                "create-linked-verifiable-presentation",
                Some(json!({"presentationIds": ["presentation-2"]})),
                204,
            )
            .await;
        fixture.validate().await;

        let requests = fixture.authorization.requests.lock().unwrap();
        for operation in [
            "identity.services.domain_linkage.create",
            "identity.services.domain_linkage.delete",
            "identity.services.linked_verifiable_presentation.create",
            "identity.services.linked_verifiable_presentation.delete",
        ] {
            assert!(requests.iter().any(|request| request.actor.as_ref().is_some_and(|actor| actor.subject == "administrator")
                && matches!(&request.operation, AuthorizationOperation::Command { command_type, .. } if *command_type == operation)));
        }
    }

    #[tokio::test(start_paused = true)]
    #[serial_test::serial]
    async fn automatic_renewal_only_runs_near_expiry_and_stops_after_removal() {
        let fixture = Fixture::new().await;
        fixture.post("create-domain-linkage", None, 204).await;
        let original_document = fixture.did().await;
        let initial_linkage = fixture.linkage().await;
        fixture.clock.fetch_add(334 * 86400, Ordering::SeqCst);
        maintain_services(&fixture.state).await.unwrap();
        assert_eq!(fixture.linkage().await, initial_linkage);
        fixture.clock.fetch_add(86400, Ordering::SeqCst);
        let maintenance = spawn_maintenance(&fixture.state);
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(3600)).await;
        for _ in 0..100 {
            tokio::task::yield_now().await;
            if fixture.linkage().await != initial_linkage {
                break;
            }
        }
        let renewed = fixture.linkage().await;
        assert_ne!(renewed, initial_linkage);
        assert_eq!(fixture.did().await, original_document);
        fixture.validate().await;
        maintain_services(&fixture.state).await.unwrap();
        assert_eq!(fixture.linkage().await, renewed);

        fixture.post("remove-domain-linkage", None, 204).await;
        fixture.clock.fetch_add(400 * 86400, Ordering::SeqCst);
        tokio::time::advance(std::time::Duration::from_secs(3600)).await;
        tokio::task::yield_now().await;
        fixture.restart().await;
        assert_eq!(
            fixture
                .request("GET", "/.well-known/did-configuration.json", None)
                .await
                .0,
            404
        );
        maintenance.abort();
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn drift_without_overwrite_authorization_fails_and_preserves_identity() {
        let fixture = Fixture::new().await;
        let original = fixture.did().await;
        let mut configuration = fixture.configuration.clone();
        configuration.public_url = "https://new.example.org/".parse().unwrap();

        let error = initialize_documents(&fixture.state, &configuration).await.unwrap_err();

        assert!(error.to_string().contains("did:web:example.org"));
        assert!(error.to_string().contains("new.example.org"));
        assert_eq!(fixture.did().await, original);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn matching_overwrite_authorization_replaces_identity_and_retains_keys() {
        let fixture = Fixture::new().await;
        let original: Value = serde_json::from_slice(&fixture.did().await).unwrap();
        let mut configuration = fixture.configuration.clone();
        configuration.public_url = "https://new.example.org/".parse().unwrap();
        configuration.overwrite_previous_did_web = Some("did:web:example.org".into());

        initialize_documents(&fixture.state, &configuration).await.unwrap();

        let replacement: Value = serde_json::from_slice(&fixture.did().await).unwrap();
        assert_eq!(replacement["id"], "did:web:new.example.org");
        let keys = |document: &Value| {
            document["verificationMethod"]
                .as_array()
                .unwrap()
                .iter()
                .map(|method| method["publicKeyJwk"].clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(keys(&replacement), keys(&original));
        let documents: Vec<Document> =
            serde_json::from_slice(&fixture.request("GET", "/unicore/v0/documents", None).await.1).unwrap();
        assert_eq!(documents.len(), 1);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn mismatched_overwrite_authorization_fails_and_preserves_identity() {
        let fixture = Fixture::new().await;
        let original = fixture.did().await;
        let mut configuration = fixture.configuration.clone();
        configuration.public_url = "https://new.example.org/".parse().unwrap();
        configuration.overwrite_previous_did_web = Some("did:web:somewhere-else.example".into());

        let error = initialize_documents(&fixture.state, &configuration).await.unwrap_err();

        assert!(error.to_string().contains("did:web:somewhere-else.example"));
        assert!(error.to_string().contains("did:web:example.org"));
        assert_eq!(fixture.did().await, original);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn verify_domain_linkage_reports_failure_before_creation() {
        let fixture = Fixture::new().await;
        let (status, body) = fixture.request("GET", "/unicore/v0/verify-domain-linkage", None).await;
        assert_eq!(status, 200);
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(result["valid"], false);
        assert_eq!(result["message"], "Domain linkage has not been created.");
        let requests = fixture.authorization.requests.lock().unwrap();
        assert!(matches!(
            &requests.last().unwrap().operation,
            AuthorizationOperation::Query { query_type }
                if *query_type
                    == std::any::type_name::<agent_identity::service::lifecycle::DomainLinkageVerification>()
        ));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn unauthorized_service_commands_do_not_mutate_identity() {
        let fixture = Fixture::new().await;
        let original = fixture.did().await;
        for (method, operation) in [
            ("POST", "create-domain-linkage"),
            ("POST", "remove-domain-linkage"),
            ("GET", "verify-domain-linkage"),
            ("POST", "remove-linked-verifiable-presentation"),
            ("POST", "create-linked-verifiable-presentation"),
        ] {
            let response = fixture
                .app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(format!("/unicore/v0/{operation}"))
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"presentationIds":["presentation-1"]}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), 401);
        }
        fixture.authorization.denied.store(true, Ordering::SeqCst);
        fixture.post("create-domain-linkage", None, 403).await;
        fixture.post("remove-domain-linkage", None, 403).await;
        assert_eq!(
            fixture
                .request("GET", "/unicore/v0/verify-domain-linkage", None)
                .await
                .0,
            403
        );
        fixture
            .post(
                "create-linked-verifiable-presentation",
                Some(json!({"presentationIds": ["presentation-1"]})),
                403,
            )
            .await;
        fixture.post("remove-linked-verifiable-presentation", None, 403).await;
        assert_eq!(fixture.did().await, original);
        assert_eq!(
            fixture
                .request("GET", "/.well-known/did-configuration.json", None)
                .await
                .0,
            404
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn competing_creates_and_renewal_removal_cannot_resurrect_linkage() {
        let fixture = Fixture::new().await;
        let original = fixture.did().await;
        let (first, second) = tokio::join!(
            fixture.request("POST", "/unicore/v0/create-domain-linkage", None),
            fixture.request("POST", "/unicore/v0/create-domain-linkage", None),
        );
        let mut statuses = [first.0, second.0];
        statuses.sort();
        assert_eq!(statuses, [204, 409]);

        fixture.clock.fetch_add(365 * 86400, Ordering::SeqCst);
        let (renewed, removed) = tokio::join!(
            maintain_services(&fixture.state),
            fixture.request("POST", "/unicore/v0/remove-domain-linkage", None),
        );
        renewed.unwrap();
        assert_eq!(removed.0, 204);
        maintain_services(&fixture.state).await.unwrap();
        assert_eq!(fixture.did().await, original);
        assert_eq!(
            fixture
                .request("GET", "/.well-known/did-configuration.json", None)
                .await
                .0,
            404
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn removals_also_strip_services_from_disabled_documents() {
        let fixture = Fixture::new().await;
        fixture.post("create-domain-linkage", None, 204).await;
        fixture
            .post(
                "create-linked-verifiable-presentation",
                Some(json!({"presentationIds": ["presentation-1"]})),
                204,
            )
            .await;
        let mut configuration = fixture.configuration.clone();
        configuration
            .did_methods
            .get_mut(&SupportedDidMethod::Web)
            .unwrap()
            .enabled = false;
        initialize_documents(&fixture.state, &configuration).await.unwrap();
        fixture.post("remove-domain-linkage", None, 204).await;
        fixture.post("remove-linked-verifiable-presentation", None, 204).await;
        let documents: Vec<Document> =
            serde_json::from_slice(&fixture.request("GET", "/unicore/v0/documents", None).await.1).unwrap();
        assert!(documents[0].document.as_ref().unwrap().service().is_empty());
        fixture.post("create-domain-linkage", None, 400).await;
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn verify_domain_linkage_reports_failure_when_nothing_is_published() {
        let mock_server = wiremock::MockServer::start().await;
        let public_url: Url = format!("http://localhost:{}/unicore/", mock_server.address().port())
            .parse()
            .unwrap();
        let fixture = Fixture::with_public_url(public_url).await;

        fixture.post("create-domain-linkage", None, 204).await;

        // No mock is registered for `/.well-known/did-configuration.json`, so wiremock 404s it,
        // simulating a domain whose linkage was never actually published (or a broken CNAME/DNS
        // setup, which fails the same way before ever reaching this handler).
        let (status, body) = fixture.request("GET", "/unicore/v0/verify-domain-linkage", None).await;
        assert_eq!(status, 200);
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(result["valid"], false);
        let message = result["message"].as_str().unwrap();
        assert!(
            message.contains("Failed to fetch the published domain linkage configuration"),
            "unexpected message: {message}"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn verify_domain_linkage_reports_success_when_the_published_configuration_resolves() {
        let mock_server = wiremock::MockServer::start().await;
        let public_url: Url = format!("http://localhost:{}/unicore/", mock_server.address().port())
            .parse()
            .unwrap();
        let fixture = Fixture::with_public_url(public_url).await;

        fixture.post("create-domain-linkage", None, 204).await;

        let did_document = fixture.did().await;
        let linkage = fixture.linkage().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/.well-known/did.json"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(did_document))
            .mount(&mock_server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/.well-known/did-configuration.json"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(linkage))
            .mount(&mock_server)
            .await;

        let (status, body) = fixture.request("GET", "/unicore/v0/verify-domain-linkage", None).await;
        assert_eq!(status, 200);
        let result: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(result, json!({"valid": true, "message": null}));
    }
}
