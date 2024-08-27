mod holder;
mod issuance;
mod verification;

use agent_holder::state::HolderState;
use agent_issuance::state::IssuanceState;
use agent_shared::{config::config, ConfigError};
use agent_verification::state::VerificationState;
use axum::{body::Bytes, extract::MatchedPath, http::Request, response::Response, Router};
use tower_http::trace::TraceLayer;
use tracing::{info_span, Span};

pub const API_VERSION: &str = "/v0";

#[derive(Default)]
pub struct ApplicationState {
    pub issuance_state: Option<IssuanceState>,
    pub holder_state: Option<HolderState>,
    pub verification_state: Option<VerificationState>,
}

pub fn app(state: ApplicationState) -> Router {
    let ApplicationState {
        issuance_state,
        holder_state,
        verification_state,
    } = state;

    let base_path = get_base_path();

    let path = |suffix: &str| -> String {
        if let Ok(base_path) = &base_path {
            format!("/{}{}", base_path, suffix)
        } else {
            suffix.to_string()
        }
    };

    Router::new()
        .nest(
            &path(Default::default()),
            Router::new()
                .merge(issuance_state.map(issuance::router).unwrap_or_default())
                .merge(holder_state.map(holder::router).unwrap_or_default())
                .merge(verification_state.map(verification::router).unwrap_or_default()),
        )
        // Trace layer
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
                    tracing::info!("Received request");
                    tracing::info!("Request Headers: {:?}", request.headers());
                })
                .on_response(|response: &Response, _latency: std::time::Duration, _span: &Span| {
                    tracing::info!("Returning {}", response.status());
                    tracing::info!("Response Headers: {:?}", response.headers());
                })
                .on_body_chunk(|chunk: &Bytes, _latency: std::time::Duration, _span: &Span| {
                    tracing::info!("Response Body: {}", std::str::from_utf8(chunk).unwrap());
                }),
        )
}

fn get_base_path() -> Result<String, ConfigError> {
    config()
        .base_path
        .clone()
        .ok_or_else(|| ConfigError::NotFound("No configuration for `base_path` found".to_string()))
        .map(|mut base_path| {
            if base_path.starts_with('/') {
                base_path.remove(0);
            }

            if base_path.ends_with('/') {
                base_path.pop();
            }

            if base_path.is_empty() {
                panic!("UNICORE__BASE_PATH can't be empty, remove or set path");
            }

            tracing::info!("Base path: {:?}", base_path);

            base_path
        })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use agent_issuance::services::test_utils::test_issuance_services;
    use agent_store::in_memory;
    use axum::routing::post;
    use oid4vci::credential_issuer::{
        credential_configurations_supported::CredentialConfigurationsSupportedObject,
        credential_issuer_metadata::CredentialIssuerMetadata,
    };
    use serde_json::json;

    pub const CREDENTIAL_CONFIGURATION_ID: &str = "badge";
    pub const OFFER_ID: &str = "00000000-0000-0000-0000-000000000000";

    lazy_static::lazy_static! {
        pub static ref BASE_URL: url::Url = url::Url::parse("https://example.com").unwrap();
        static ref CREDENTIAL_CONFIGURATIONS_SUPPORTED: HashMap<String, CredentialConfigurationsSupportedObject> =
            vec![(
                "0".to_string(),
                serde_json::from_value(json!({
                    "format": "jwt_vc_json",
                    "cryptographic_binding_methods_supported": [
                        "did:key",
                    ],
                    "credential_signing_alg_values_supported": [
                        "EdDSA"
                    ],
                    "credential_definition":{
                        "type": [
                            "VerifiableCredential",
                            "OpenBadgeCredential"
                        ]
                    },
                    "proof_types_supported": [
                        "jwt"
                    ]
                }
                ))
                .unwrap()
            )]
            .into_iter()
            .collect();
        pub static ref CREDENTIAL_ISSUER_METADATA: CredentialIssuerMetadata = CredentialIssuerMetadata {
            credential_issuer: BASE_URL.clone(),
            credential_endpoint: BASE_URL.join("credential").unwrap(),
            batch_credential_endpoint: Some(BASE_URL.join("batch_credential").unwrap()),
            credential_configurations_supported: CREDENTIAL_CONFIGURATIONS_SUPPORTED.clone(),
            ..Default::default()
        };
    }

    async fn handler() {}

    #[tokio::test]
    #[should_panic]
    async fn test_base_path_routes() {
        let issuance_state = in_memory::issuance_state(test_issuance_services(), Default::default()).await;
        std::env::set_var("UNICORE__BASE_PATH", "unicore");
        let router = app(ApplicationState {
            issuance_state: Some(issuance_state),
            ..Default::default()
        });

        let _ = router.route("/auth/token", post(handler));
    }
}
