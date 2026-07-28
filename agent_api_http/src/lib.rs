pub mod public;
pub mod v0;

pub mod error;
pub mod extractors;
pub mod handlers;
pub mod metrics;
pub mod utils;

use agent_authorization::state::AuthorizationState;
use agent_holder::state::HolderState;
use agent_identity::state::IdentityState;
use agent_issuance::state::IssuanceState;
use agent_library::state::LibraryState;
use agent_shared::config::config;
use agent_verification::state::VerificationState;
use axum::{
    body::{Body, Bytes},
    extract::{MatchedPath, Request, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use http::HeaderMap;
use http_body_util::BodyExt as _;
use hyper::StatusCode;
use shared_kernel::authorization::{Actor, ActorExtractor, ToActor};
use shared_kernel::event_bus::EventBusHandle;
use std::{sync::Arc, time::Duration};
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::{debug, info, info_span, Span};

pub const API_VERSION: &str = "/v0";

pub const DOCUMENTATION_URL: &str = "https://beta.docs.impierce.com/unicore/";

#[derive(Default)]
pub struct ApiState {
    pub identity_state: Option<Arc<IdentityState>>,
    pub library_state: Option<Arc<LibraryState>>,
    pub authorization_state: Option<Arc<AuthorizationState>>,
    pub issuance_state: Option<Arc<IssuanceState>>,
    pub holder_state: Option<Arc<HolderState>>,
    pub verification_state: Option<Arc<VerificationState>>,
    pub event_bus: Option<EventBusHandle>,
}

/// Build the top-level API router.
///
/// This merges the routers for each bounded context that has state available,
/// installs actor extraction middleware, and attaches request tracing/logging.
/// When the configured application URL includes a non-root base path, the
/// router is nested under that path.
pub fn app<E>(
    ApiState {
        identity_state,
        library_state,
        authorization_state,
        issuance_state,
        holder_state,
        verification_state,
        event_bus,
    }: ApiState,
    actor_extractor: Arc<E>,
) -> Router
where
    E: ActorExtractor,
{
    let events_router = event_bus.map(v0::events::router).unwrap_or_default();

    let app = Router::new()
        .merge(identity_state.map(v0::identity::router).unwrap_or_default())
        .merge(library_state.clone().map(v0::library::router).unwrap_or_default())
        .merge(
            authorization_state
                // The `IssuanceState` is cloned here to ensure that the authorization router can access it. This is
                // necessary since for the Pre-Authorized Code flow, the Token Endpoint requires a shared state with
                // the Issuance Bounded Context.
                .zip(issuance_state.clone())
                .map(v0::authorization::router)
                .unwrap_or_default(),
        )
        .merge(
            issuance_state
                .zip(library_state.clone())
                .map(v0::issuance::router)
                .unwrap_or_default(),
        )
        .merge(holder_state.map(v0::holder::router).unwrap_or_default())
        .merge(verification_state.map(v0::verification::router).unwrap_or_default())
        .merge(events_router)
        .merge(public::router())
        .layer(middleware::from_fn_with_state(actor_extractor, extract_actor::<E>))
        // Trace layers
        .layer(
            ServiceBuilder::new()
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(|request: &Request<_>| {
                            let path = request
                                .extensions()
                                .get::<MatchedPath>()
                                .map(MatchedPath::as_str)
                                .unwrap_or_else(|| request.uri().path());
                            info_span!(
                                "HTTP Request",
                                method = ?request.method(),
                                path,
                            )
                        })
                        .on_request(|request: &Request<_>, _span: &Span| {
                            info!("Received request");
                            info!("Request Headers: {:?}", request.headers());
                        })
                        .on_response(|response: &Response, _latency: Duration, _span: &Span| {
                            info!("Returning {}", response.status());
                            info!("Response Headers: {:?}", response.headers());
                        })
                        .on_body_chunk(|chunk: &Bytes, _latency: Duration, _span: &Span| {
                            if let Ok(response_body) = std::str::from_utf8(chunk) {
                                info!("Response Body: {response_body}");
                            }
                        }),
                )
                .layer(middleware::from_fn(log_request_body)),
        );

    let application_base_path = config().application_url.path().to_string();

    // Note: since version 0.8 axum does not allow nesting routers with an empty base path. We must explicitly check
    // for an empty base path before nesting.
    if application_base_path == "/" {
        app
    } else {
        // TODO: This breaks Domain Linkage. We need to fix this.
        Router::new().nest(&application_base_path, app)
    }
}

// This middleware logs the request body before passing it on.
async fn log_request_body(request: Request, next: Next) -> Result<impl IntoResponse, Response> {
    let request = buffer_request_body(request).await?;

    Ok(next.run(request).await)
}

// Buffer the request body so it can be logged.
async fn buffer_request_body(request: Request) -> Result<Request, Response> {
    let (parts, body) = request.into_parts();

    debug!("Path segments and query string: `{}`", parts.uri);

    // Convert the request body into bytes.
    let bytes = body
        .collect()
        .await
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()).into_response())?
        .to_bytes();

    let _ = serde_json::from_slice(&bytes)
        .and_then(|json_value: serde_json::Value| serde_json::to_string_pretty(&json_value))
        .map(|pretty_json| info!("Request Body: {}", pretty_json));

    Ok(Request::from_parts(parts, Body::from(bytes)))
}

/// Adapter that lets the actor extractor read values from HTTP headers.
struct HttpActorInput<'a> {
    headers: &'a HeaderMap,
}

impl<'a> HttpActorInput<'a> {
    /// Create a header-backed actor input used by the actor extractor.
    fn from_headers(headers: &'a HeaderMap) -> Self {
        Self { headers }
    }
}

impl ToActor for HttpActorInput<'_> {
    /// Raw HTTP credentials are not stable actor identifiers.
    ///
    /// Actor extractors should read credentials with [`ToActor::auth_value`] and map them to a
    /// non-sensitive subject before returning an [`Actor`].
    fn to_actor(&self) -> Option<Actor> {
        None
    }

    /// Read the header identified by `key` as a UTF-8 string slice.
    fn auth_value(&self, key: &str) -> Option<&str> {
        self.headers.get(key).and_then(|value| value.to_str().ok())
    }
}

/// Extract an optional actor from the request headers and store it in the request extensions when present.
pub async fn extract_actor<E>(State(actor_extractor): State<Arc<E>>, mut request: Request, next: Next) -> Response
where
    E: ActorExtractor,
{
    let input = HttpActorInput::from_headers(request.headers());

    if let Some(actor) = actor_extractor.extract_actor(&input).await {
        request.extensions_mut().insert(actor);
    }

    next.run(request).await
}

/// Require a valid actor in the request headers, returning `401 Unauthorized` when none is present.
///
/// When an actor is found, it is inserted into the request extensions before the request is forwarded to the next handler.
pub async fn require_actor<E>(
    State(actor_extractor): State<Arc<E>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode>
where
    E: ActorExtractor,
{
    let input = HttpActorInput::from_headers(request.headers());

    if let Some(actor) = actor_extractor.extract_actor(&input).await {
        request.extensions_mut().insert(actor);
    } else {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extractors::RequestActor;
    use agent_shared::config::config;
    use axum::{body::Body, routing::get};
    use http::header::AUTHORIZATION;
    use http::Request;
    use oid4vci::credential_issuer::{
        credential_configurations_supported::CredentialConfigurationsSupportedObject,
        credential_issuer_metadata::CredentialIssuerMetadata,
    };
    use serde_json::json;
    use shared_kernel::authorization::NoActorExtractor;
    use std::collections::HashMap;
    use tower::ServiceExt;

    pub const OFFER_ID: &str = "00000000-0000-0000-0000-000000000000";
    pub const TEMPLATE_ID: &str = "001";

    lazy_static::lazy_static! {
        static ref CREDENTIAL_CONFIGURATIONS_SUPPORTED: HashMap<String, CredentialConfigurationsSupportedObject> =
            vec![(
                TEMPLATE_ID.to_string(),
                serde_json::from_value(json!({
                    "format": "jwt_vc_json",
                    "cryptographic_binding_methods_supported": [
                        "did:jwk",
                        "did:key",
                    ],
                    "credential_signing_alg_values_supported": [
                        "ES256",
                        "EdDSA"
                    ],
                    "credential_definition":{
                        "type": [
                            "VerifiableCredential"
                        ]
                    },
                    "proof_types_supported": {
                        "jwt": {
                            "proof_signing_alg_values_supported": [
                                "ES256",
                                "EdDSA"
                            ],
                        }
                    },
                    "credential_metadata": {
                        "display": [
                            {
                                "name": "Verifiable Credential",
                                "locale": "en",
                                "logo": {
                                    "uri": "https://www.impierce.com/external/impierce-logo.png",
                                    "alt_text": "Impierce Logo",
                                }
                            }
                        ]
                    }}
                ))
                .unwrap()
            )]
            .into_iter()
            .collect();
        pub static ref CREDENTIAL_ISSUER_METADATA: CredentialIssuerMetadata = CredentialIssuerMetadata {
            credential_issuer: config().public_url.clone(),
            credential_endpoint: config().public_url.join("openid4vci/credential").unwrap(),
            nonce_endpoint: Some(config().public_url.join("openid4vci/nonce").unwrap()),
            credential_configurations_supported: CREDENTIAL_CONFIGURATIONS_SUPPORTED.clone(),
            display: Some(vec![json!({
                "name": "UniCore",
                "locale": "en",
                "logo": {
                    "uri": "https://www.impierce.com/external/impierce-icon.png",
                    "alt_text": "Impierce Icon",
                }
            })]),
            ..Default::default()
        };
    }

    #[derive(Clone)]
    struct MappingActorExtractor;

    #[async_trait::async_trait]
    impl ActorExtractor for MappingActorExtractor {
        async fn extract_actor(&self, input: &dyn ToActor) -> Option<Actor> {
            input
                .bearer_token()
                .filter(|token| *token == "valid-token")
                .map(|_| Actor {
                    subject: "user@example.test".to_string(),
                })
        }
    }

    #[derive(Clone)]
    struct CustomHeaderActorExtractor;

    #[async_trait::async_trait]
    impl ActorExtractor for CustomHeaderActorExtractor {
        async fn extract_actor(&self, input: &dyn ToActor) -> Option<Actor> {
            input
                .auth_value("x-custom-actor-token")
                .filter(|token| *token == "valid-token")
                .map(|_| Actor {
                    subject: "custom@example.test".to_string(),
                })
        }
    }

    async fn actor_subject(RequestActor(actor): RequestActor) -> String {
        actor
            .map(|actor| actor.subject)
            .unwrap_or_else(|| "anonymous".to_string())
    }

    #[tokio::test]
    async fn actor_extraction_middleware_stores_mapped_actor_in_request_extensions() {
        let app = Router::new()
            .route("/", get(actor_subject))
            .layer(middleware::from_fn_with_state(
                Arc::new(MappingActorExtractor),
                extract_actor::<MappingActorExtractor>,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(AUTHORIZATION, "Bearer valid-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(body.as_ref(), b"user@example.test");
    }

    #[tokio::test]
    async fn actor_extractor_can_read_custom_auth_values() {
        let app = Router::new()
            .route("/", get(actor_subject))
            .layer(middleware::from_fn_with_state(
                Arc::new(CustomHeaderActorExtractor),
                extract_actor::<CustomHeaderActorExtractor>,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("x-custom-actor-token", "valid-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(body.as_ref(), b"custom@example.test");
    }

    #[tokio::test]
    async fn no_actor_extractor_stores_anonymous_actor_extension() {
        let app = Router::new()
            .route("/", get(actor_subject))
            .layer(middleware::from_fn_with_state(
                Arc::new(NoActorExtractor),
                extract_actor::<NoActorExtractor>,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(AUTHORIZATION, "Bearer valid-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(body.as_ref(), b"anonymous");
    }
}
