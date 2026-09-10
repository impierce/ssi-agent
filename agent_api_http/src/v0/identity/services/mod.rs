pub mod linked_domains;
pub mod linked_vp;

use crate::extractors::RequestActor;
use crate::handlers::query_handler;
use agent_identity::{
    service::aggregate::{Service, ServiceResource},
    state::IdentityState,
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use identity_document::service::Service as DocumentService;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize, utoipa::ToSchema)]
struct ServiceResponse {
    #[serde(rename = "id")]
    service_id: String,
    /// TODO: Replace this generic object schema with a schema for `identity_document::service::Service`.
    #[schema(value_type = Option<Object>)]
    service: Option<DocumentService>,
    presentation_ids: Vec<String>,
    /// TODO: Replace this generic object schema with a schema for `DomainLinkageConfiguration`.
    #[schema(value_type = Option<Object>)]
    resource: Option<ServiceResource>,
}

impl From<Service> for ServiceResponse {
    fn from(service: Service) -> Self {
        Self {
            service_id: service.service_id,
            service: service.service,
            presentation_ids: service.presentation_ids,
            resource: service.resource,
        }
    }
}

/// List identity services
#[utoipa::path(
    get,
    path = "/services",
    operation_id = "list_identity_services",
    tags = ["Identity"],
    responses(
        (status = 200, description = "Identity services", body = [ServiceResponse]),
    )
)]
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
    .map(|all_services_view| {
        all_services_view
            .services
            .into_values()
            .filter(|service| !service.is_deleted)
            .map(ServiceResponse::from)
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_services)).into_response())
}

/// Get an identity service
#[utoipa::path(
    get,
    path = "/services/{service_id}",
    operation_id = "get_identity_service",
    tags = ["Identity"],
    params(
        ("service_id" = String, Path, description = "Identity service ID"),
    ),
    responses(
        (status = 200, description = "Identity service", body = ServiceResponse),
        (status = 404, description = "Identity service not found"),
    )
)]
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
    .filter(|service_view| !service_view.is_deleted)
    .map(|service_view| (StatusCode::OK, Json(ServiceResponse::from(service_view))).into_response())
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[cfg(test)]
mod tests {
    use agent_identity::{
        dns::CnameResolver,
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

    /// A stand-in for real DNS, so that verification tests can decide what a linked domain's `CNAME`
    /// chain looks like without touching the network.
    #[derive(Default)]
    struct StubCnameResolver {
        chain: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl CnameResolver for StubCnameResolver {
        async fn cname_chain(&self, _host: &str) -> Result<Vec<String>, String> {
            Ok(self.chain.lock().unwrap().clone())
        }
    }

    struct Fixture {
        state: Arc<IdentityState>,
        app: Router,
        configuration: ApplicationConfiguration,
        clock: Arc<AtomicI64>,
        authorization: Arc<RecordingAuthorization>,
        cname: Arc<StubCnameResolver>,
    }

    impl Fixture {
        async fn new() -> Self {
            Self::with_public_url("https://example.org/unicore/".parse().unwrap()).await
        }

        /// Like [`Fixture::new`], but the deployment identity and its published `.well-known`
        /// resources live at `public_url` instead of the fixed `https://example.org/unicore/`. Used
        /// to point linked-domain self-verification at a mock server it can actually reach.
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
            let cname = Arc::new(StubCnameResolver::default());
            let mut services = IdentityServices::new(Arc::new(Subject::test_subject().await));
            services.public_url = configuration.public_url.clone();
            services.cname_resolver = cname.clone();
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
                cname,
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

        /// Links `public_url`'s own origin, for the many tests that are not about *which* domain is
        /// linked and just need a linked one.
        async fn link_own_origin(&self, expected: u16) {
            self.add_linked_domains(&[self.configuration.public_url.as_str()], expected)
                .await;
        }

        async fn add_linked_domains(&self, origins: &[&str], expected: u16) {
            self.post("add-linked-domains", Some(json!({ "origins": origins })), expected)
                .await;
        }

        async fn remove_linked_domains(&self, origins: &[&str], expected: u16) {
            self.post("remove-linked-domains", Some(json!({ "origins": origins })), expected)
                .await;
        }

        /// Points the stubbed resolver at `target`, as a correctly configured `CNAME` would.
        fn cname_points_to(&self, target: &str) {
            *self.cname.chain.lock().unwrap() = vec![format!("{target}.")];
        }

        async fn verify(&self) -> Value {
            let (status, body) = self.request("GET", "/unicore/v0/verify-linked-domains", None).await;
            assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
            serde_json::from_slice(&body).unwrap()
        }

        /// Every origin currently named by the published `LinkedDomains` service entry.
        async fn published_origins(&self) -> Vec<String> {
            let document: Value = serde_json::from_slice(&self.did().await).unwrap();
            let endpoint = document["service"]
                .as_array()
                .unwrap()
                .iter()
                .find(|service| service["type"] == "LinkedDomains")
                .map(|service| service["serviceEndpoint"].clone())
                .expect("a LinkedDomains service is published");

            // The specification allows either form, and UniCore writes whichever fits.
            match endpoint {
                Value::String(origin) => vec![origin],
                Value::Object(map) => map["origins"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|origin| origin.as_str().unwrap().to_string())
                    .collect(),
                other => panic!("unexpected serviceEndpoint: {other}"),
            }
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

        /// Validates the published linkage exactly as an external verifier would, once per linked
        /// origin, and confirms an origin that was never linked is rejected.
        ///
        /// Both halves matter: `validate_linkage` succeeds as soon as *one* credential matches the
        /// origin it is given, so checking only the origins that should pass would not catch a
        /// configuration that quietly covers the wrong domains.
        async fn validate(&self, origins: &[&str]) {
            let document: CoreDocument = serde_json::from_slice(&self.did().await).unwrap();
            let configuration: DomainLinkageConfiguration = serde_json::from_slice(&self.linkage().await).unwrap();
            let now = Timestamp::from_unix(self.clock.load(Ordering::SeqCst)).unwrap();
            let options = JwtCredentialValidationOptions::default()
                .latest_issuance_date(now)
                .earliest_expiry_date(now);
            let validator = JwtDomainLinkageValidator::with_signature_verifier(SignatureVerifier);

            for origin in origins {
                validator
                    .validate_linkage(&document, &configuration, &origin.parse().unwrap(), &options)
                    .unwrap_or_else(|error| panic!("expected '{origin}' to be linked: {error}"));
            }

            assert!(
                validator
                    .validate_linkage(
                        &document,
                        &configuration,
                        &"https://never-linked.example".parse().unwrap(),
                        &options,
                    )
                    .is_err(),
                "an origin that was never linked must not validate"
            );
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
        // Unlinking a domain that is not linked is a no-op.
        fixture
            .remove_linked_domains(&[fixture.configuration.public_url.as_str()], 204)
            .await;
        fixture.post("remove-linked-verifiable-presentation", None, 404).await;
        fixture
            .post("services/linked-vp", Some(json!({"presentationIds": []})), 405)
            .await;

        fixture.link_own_origin(204).await;
        // Linking the same origin again changes nothing.
        fixture.link_own_origin(204).await;
        fixture
            .validate(&[fixture.configuration.public_url.origin().ascii_serialization().as_str()])
            .await;
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

        fixture
            .remove_linked_domains(&[fixture.configuration.public_url.as_str()], 204)
            .await;
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
        fixture.link_own_origin(204).await;
        fixture
            .post(
                "create-linked-verifiable-presentation",
                Some(json!({"presentationIds": ["presentation-2"]})),
                204,
            )
            .await;
        fixture
            .validate(&[fixture.configuration.public_url.origin().ascii_serialization().as_str()])
            .await;

        let requests = fixture.authorization.requests.lock().unwrap();
        for operation in [
            "identity.services.linked_domains.add",
            "identity.services.linked_domains.remove",
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
        fixture.link_own_origin(204).await;
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
        fixture
            .validate(&[fixture.configuration.public_url.origin().ascii_serialization().as_str()])
            .await;
        maintain_services(&fixture.state).await.unwrap();
        assert_eq!(fixture.linkage().await, renewed);

        fixture
            .remove_linked_domains(&[fixture.configuration.public_url.as_str()], 204)
            .await;
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
    async fn unauthorized_service_commands_do_not_mutate_identity() {
        let fixture = Fixture::new().await;
        let original = fixture.did().await;
        // Authorization is checked inside the handler, after the request body is parsed, so a body
        // that doesn't match what the operation expects would be rejected for that reason instead —
        // each operation gets a body shaped like a real request would be, to prove it's specifically
        // the missing authorization being rejected.
        for (method, operation, body) in [
            ("POST", "add-linked-domains", r#"{"origins":["https://example.org"]}"#),
            (
                "POST",
                "remove-linked-domains",
                r#"{"origins":["https://example.org"]}"#,
            ),
            ("GET", "verify-linked-domains", "{}"),
            ("POST", "remove-linked-verifiable-presentation", "{}"),
            (
                "POST",
                "create-linked-verifiable-presentation",
                r#"{"presentationIds":["presentation-1"]}"#,
            ),
        ] {
            let response = fixture
                .app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(format!("/unicore/v0/{operation}"))
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), 401, "operation: {operation}");
        }
        fixture.authorization.denied.store(true, Ordering::SeqCst);
        fixture.link_own_origin(403).await;
        fixture
            .remove_linked_domains(&[fixture.configuration.public_url.as_str()], 403)
            .await;
        assert_eq!(
            fixture
                .request("GET", "/unicore/v0/verify-linked-domains", None)
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
        let origins = Some(json!({"origins": [fixture.configuration.public_url.as_str()]}));
        let (first, second) = tokio::join!(
            fixture.request("POST", "/unicore/v0/add-linked-domains", origins.clone()),
            fixture.request("POST", "/unicore/v0/add-linked-domains", origins.clone()),
        );
        // Linking is idempotent, so racing requests both succeed.
        assert_eq!([first.0, second.0], [204, 204]);

        fixture.clock.fetch_add(365 * 86400, Ordering::SeqCst);
        let (renewed, removed) = tokio::join!(
            maintain_services(&fixture.state),
            fixture.request("POST", "/unicore/v0/remove-linked-domains", origins),
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
        fixture.link_own_origin(204).await;
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
        fixture
            .remove_linked_domains(&[fixture.configuration.public_url.as_str()], 204)
            .await;
        fixture.post("remove-linked-verifiable-presentation", None, 204).await;
        let documents: Vec<Document> =
            serde_json::from_slice(&fixture.request("GET", "/unicore/v0/documents", None).await.1).unwrap();
        assert!(documents[0].document.as_ref().unwrap().service().is_empty());
        fixture.link_own_origin(400).await;
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn verify_reports_failure_before_any_domain_is_linked() {
        let fixture = Fixture::new().await;

        let result = fixture.verify().await;

        assert_eq!(result["valid"], false);
        assert_eq!(result["message"], "No domains are linked.");
        assert_eq!(result["origins"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn verify_reports_the_missing_cname_when_a_domain_publishes_nothing() {
        let mock_server = wiremock::MockServer::start().await;
        let public_url: Url = format!("http://localhost:{}/unicore/", mock_server.address().port())
            .parse()
            .unwrap();
        let fixture = Fixture::with_public_url(public_url).await;

        fixture.link_own_origin(204).await;

        // No mock is registered for `/.well-known/did-configuration.json`, so wiremock 404s it,
        // simulating a domain whose linkage was never actually published. The stubbed resolver
        // reports no CNAME, which is the usual underlying cause and is surfaced alongside it.
        let result = fixture.verify().await;

        assert_eq!(result["valid"], false);
        let origin = &result["origins"][0];
        assert_eq!(origin["valid"], false);
        assert_eq!(origin["linkage_valid"], false);
        assert_eq!(origin["dns"]["points_here"], false);
        let message = origin["message"].as_str().unwrap();
        assert!(
            message.contains("Failed to fetch the published domain linkage configuration"),
            "unexpected message: {message}"
        );
        assert!(
            message.contains("No CNAME record found"),
            "unexpected message: {message}"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn verify_reports_success_when_the_published_configuration_resolves() {
        let mock_server = wiremock::MockServer::start().await;
        let public_url: Url = format!("http://localhost:{}/unicore/", mock_server.address().port())
            .parse()
            .unwrap();
        let fixture = Fixture::with_public_url(public_url).await;

        fixture.link_own_origin(204).await;
        fixture.cname_points_to("localhost");

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

        let result = fixture.verify().await;

        assert_eq!(result["valid"], true);
        let origin = &result["origins"][0];
        assert_eq!(origin["valid"], true);
        assert_eq!(origin["linkage_valid"], true);
        assert_eq!(origin["dns"]["points_here"], true);
        assert!(origin["message"].is_null());
    }

    /// A linked domain is verified even when its DNS has no `CNAME` — an apex domain cannot have one,
    /// so the cryptographic proof, not the DNS diagnostic, decides the outcome.
    #[tokio::test]
    #[serial_test::serial]
    async fn a_working_domain_without_a_cname_still_verifies() {
        let mock_server = wiremock::MockServer::start().await;
        let public_url: Url = format!("http://localhost:{}/unicore/", mock_server.address().port())
            .parse()
            .unwrap();
        let fixture = Fixture::with_public_url(public_url).await;

        fixture.link_own_origin(204).await;
        // Left as the default empty chain: no CNAME record at all.

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

        let result = fixture.verify().await;

        assert_eq!(result["valid"], true);
        assert_eq!(result["origins"][0]["valid"], true);
        assert_eq!(result["origins"][0]["dns"]["points_here"], false);
    }

    /// Which domains are linked is independent of the deployment's own identity: linking two domains
    /// that are both different from `public_url` publishes a credential for each, names both in the
    /// `LinkedDomains` service, and verifies each against its own host — the deployment server, which
    /// serves `did.json`, is never asked for a `did-configuration.json` at all.
    #[tokio::test]
    #[serial_test::serial]
    async fn linked_domains_are_independent_of_public_url() {
        let deployment_mock = wiremock::MockServer::start().await;
        let public_url: Url = format!("http://localhost:{}/unicore/", deployment_mock.address().port())
            .parse()
            .unwrap();
        let fixture = Fixture::with_public_url(public_url).await;

        let first_mock = wiremock::MockServer::start().await;
        let second_mock = wiremock::MockServer::start().await;
        let first = format!("http://localhost:{}", first_mock.address().port());
        let second = format!("http://localhost:{}", second_mock.address().port());

        fixture.add_linked_domains(&[&first, &second], 204).await;
        fixture.cname_points_to("localhost");

        // Both origins are named by the service entry, in the array form.
        let mut published = fixture.published_origins().await;
        published.sort();
        let mut expected = vec![format!("{first}/"), format!("{second}/")];
        expected.sort();
        assert_eq!(published, expected);

        // One credential per origin, each claiming its own — never `public_url`.
        let linkage: Value = serde_json::from_slice(&fixture.linkage().await).unwrap();
        let mut claimed: Vec<String> = linkage["linked_dids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|jwt| {
                decode_unverified_jwt_claims(jwt.as_str().unwrap())["vc"]["credentialSubject"]["origin"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        claimed.sort();
        claimed.dedup();
        assert_eq!(claimed, expected);

        // The DID still resolves through the deployment itself.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/.well-known/did.json"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(fixture.did().await))
            .mount(&deployment_mock)
            .await;

        // Each linked domain serves the same document, exactly as CNAMEd domains would.
        for mock in [&first_mock, &second_mock] {
            wiremock::Mock::given(wiremock::matchers::method("GET"))
                .and(wiremock::matchers::path("/.well-known/did-configuration.json"))
                .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(fixture.linkage().await))
                .mount(mock)
                .await;
        }

        let result = fixture.verify().await;
        assert_eq!(result["valid"], true, "{result}");
        assert_eq!(result["origins"].as_array().unwrap().len(), 2);
        // A verifier of one origin accepts the shared document even though it also carries the other
        // origin's credential, and neither origin is left unverified.
        fixture.validate(&[&first, &second]).await;

        // Unlinking one leaves the other verifiable and returns the endpoint to the flattened form.
        fixture.remove_linked_domains(&[&second], 204).await;
        assert_eq!(fixture.published_origins().await, vec![format!("{first}/")]);
        fixture.validate(&[&first]).await;

        let result = fixture.verify().await;
        assert_eq!(result["origins"].as_array().unwrap().len(), 1);
        assert_eq!(result["origins"][0]["origin"], format!("{first}/"));

        // Unlinking the last one withdraws the published configuration entirely.
        fixture.remove_linked_domains(&[&first], 204).await;
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
    async fn linking_rejects_unusable_origins() {
        let fixture = Fixture::new().await;

        for origin in ["https://127.0.0.1", "not a domain!!", "data:text/plain,hello"] {
            fixture.add_linked_domains(&[origin], 400).await;
        }
        // One bad entry rejects the whole request, rather than linking the good ones.
        fixture
            .add_linked_domains(&["https://good.example", "https://127.0.0.1"], 400)
            .await;
        assert_eq!(
            fixture
                .request("GET", "/.well-known/did-configuration.json", None)
                .await
                .0,
            404
        );
        // An empty list is not a way to link nothing.
        fixture.add_linked_domains(&[], 400).await;
        fixture.remove_linked_domains(&[], 400).await;
    }

    fn decode_unverified_jwt_claims(jwt: &str) -> Value {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let payload = jwt.split('.').nth(1).unwrap();
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap()
    }
}
