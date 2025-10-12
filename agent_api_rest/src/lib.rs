pub mod authorization;
pub mod holder;
pub mod identity;
pub mod issuance;
pub mod metrics;
pub mod verification;

pub mod error;
pub mod handlers;
pub mod utils;

use agent_authorization::state::AuthorizationState;
use agent_holder::state::HolderState;
use agent_identity::state::IdentityState;
use agent_issuance::state::IssuanceState;
use agent_shared::config::config;
use agent_verification::state::VerificationState;
use axum::{
    body::{Body, Bytes},
    extract::{MatchedPath, Request},
    middleware,
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};
use http_body_util::BodyExt as _;
use hyper::StatusCode;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{debug, info, info_span, Span};

pub const API_VERSION: &str = "/v0";

pub const DOCUMENTATION_URL: &str = "https://beta.docs.impierce.com/unicore/";

#[derive(Default)]
pub struct ApplicationState {
    pub identity_state: Option<IdentityState>,
    pub authorization_state: Option<AuthorizationState>,
    pub issuance_state: Option<IssuanceState>,
    pub holder_state: Option<HolderState>,
    pub verification_state: Option<VerificationState>,
}

pub fn app(
    ApplicationState {
        identity_state,
        authorization_state,
        issuance_state,
        holder_state,
        verification_state,
    }: ApplicationState,
) -> Router {
    let app = Router::new()
        .merge(identity_state.map(identity::router).unwrap_or_default())
        .merge(
            authorization_state
                // The `IssuanceState` is cloned here to ensure that the authorization router can access it. This is
                // necessary since for the Pre-Authorized Code flow, the Token Endpoint requires a shared state with
                // the Issuance Bounded Context.
                .zip(issuance_state.clone())
                .map(authorization::router)
                .unwrap_or_default(),
        )
        .merge(issuance_state.map(issuance::router).unwrap_or_default())
        .merge(holder_state.map(holder::router).unwrap_or_default())
        .merge(verification_state.map(verification::router).unwrap_or_default())
        // Trace layers
        .layer(
            ServiceBuilder::new()
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(|request: &Request<_>| {
                            let path = request.extensions().get::<MatchedPath>().map(MatchedPath::as_str);
                            info_span!(
                                "HTTP Request ",
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
    let app = if application_base_path == "/" {
        app
    } else {
        // TODO: This breaks Domain Linkage. We need to fix this.
        Router::new().nest(&application_base_path, app)
    };

    // CORS
    if config().cors_enabled {
        info!("CORS (permissive) enabled for all routes");
        app.layer(CorsLayer::permissive())
    } else {
        app
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

#[cfg(test)]
mod tests {
    use agent_shared::config::config;
    use oid4vci::credential_issuer::{
        credential_configurations_supported::CredentialConfigurationsSupportedObject,
        credential_issuer_metadata::CredentialIssuerMetadata,
    };
    use serde_json::json;
    use std::collections::HashMap;

    pub const OFFER_ID: &str = "00000000-0000-0000-0000-000000000000";

    lazy_static::lazy_static! {
        static ref CREDENTIAL_CONFIGURATIONS_SUPPORTED: HashMap<String, CredentialConfigurationsSupportedObject> =
            vec![(
                "001".to_string(),
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
                }
                ))
                .unwrap()
            ),
            (
                "002".to_string(),
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
                    "display": [
                        {
                            "name": "Verifiable Credential",
                            "locale": "en",
                            "logo": {
                                "uri": "https://www.impierce.com/external/impierce-logo.png",
                                "alt_text": "Impierce Logo",
                            }
                        }
                    ],
                    "authorization": {
                        "pre_authorized": false
                    }
                }
                ))
                .unwrap()
            )]
            .into_iter()
            .collect();
        pub static ref CREDENTIAL_ISSUER_METADATA: CredentialIssuerMetadata = CredentialIssuerMetadata {
            credential_issuer: config().public_url.clone(),
            credential_endpoint: config().public_url.join("openid4vci/credential").unwrap(),
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
}
