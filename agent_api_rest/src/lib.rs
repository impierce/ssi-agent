pub mod holder;
pub mod issuance;
pub mod openapi;
pub mod verification;

use agent_holder::state::HolderState;
use agent_issuance::state::IssuanceState;
use agent_shared::{config::config, ConfigError};
use agent_verification::state::VerificationState;
use axum::{body::Bytes, extract::MatchedPath, http::Request, response::Response, Router};
use tower_http::trace::TraceLayer;
use tracing::{info_span, Span};
use utoipa::{openapi::ServerBuilder, OpenApi};
use utoipa_scalar::{Scalar, Servable};

use crate::openapi::{HolderApi, IssuanceApi, VerificationApi, WellKnownApi};

pub const API_VERSION: &str = "/v0";

#[derive(Default)]
pub struct ApplicationState {
    pub issuance_state: Option<IssuanceState>,
    pub holder_state: Option<HolderState>,
    pub verification_state: Option<VerificationState>,
}

pub fn app(
    ApplicationState {
        issuance_state,
        holder_state,
        verification_state,
    }: ApplicationState,
) -> Router {
    Router::new()
        .nest(
            &get_base_path().unwrap_or_default(),
            Router::new()
                .merge(issuance_state.map(issuance::router).unwrap_or_default())
                .merge(holder_state.map(holder::router).unwrap_or_default())
                .merge(verification_state.map(verification::router).unwrap_or_default())
                // API Docs
                .merge(Scalar::with_url(
                    format!("{}/api-reference", API_VERSION),
                    patch_generated_openapi(ApiDoc::openapi()),
                )),
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

            format!("/{}", base_path)
        })
}

// #[derive(OpenApi)]
// #[openapi(modifiers(), nest((path = "/v0/todos", api = WellKnownApi)), tags((name = "well-known")))]
// struct ApiDoc;

#[derive(utoipa::OpenApi)]
#[openapi(
        // modifiers(),
        nest(
            (path = "/.well-known", api = WellKnownApi),
            (path = "/v0", api = IssuanceApi),
            (path = "/v0", api = VerificationApi),
            (path = "/v0", api = HolderApi)
        ),
        paths(
            crate::holder::openid4vci::offers,
            crate::issuance::credential_issuer::credential::credential,
        ),
        // paths(
        //     crate::issuance::credential_issuer::CredentialApi
        // ),
        tags(
            // (name = "todo", description = "Todo items management API"),
            (name = "OpenID4VCI", description = "All operations revolved around the OpenID4VCI standard.", external_docs(url = "https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html", description = "OpenID for Verifiable Credential Issuance")),
            (name = "Well-Known", description = "Well-known endpoints provide metadata about the server."),
        )
    )]
pub struct ApiDoc;

pub fn patch_generated_openapi(mut openapi: utoipa::openapi::OpenApi) -> utoipa::openapi::OpenApi {
    openapi.info.title = "UniCore HTTP API".into();
    openapi.info.description = Some("Full HTTP API reference for the UniCore SSI Agent".to_string());
    // openapi.info.version = "1.0.0-alpha.1".into(); // can this be determined or does it need to be removed from the openapi.yaml?
    openapi.info.version = "".into();
    openapi.servers = vec![ServerBuilder::new()
        .url("https://arty-aragorn.agent-dev.impierce.com")
        .build()]
    .into();
    openapi
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_secret_manager::service::Service;
    use agent_store::in_memory;
    use axum::routing::post;
    use oid4vci::credential_issuer::{
        credential_configurations_supported::CredentialConfigurationsSupportedObject,
        credential_issuer_metadata::CredentialIssuerMetadata,
    };
    use serde_json::json;
    use utoipa::OpenApi;

    use crate::{app, ApiDoc};
    use std::collections::HashMap;

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
    async fn generate_openapi_file() {
        let yaml_value = patch_generated_openapi(ApiDoc::openapi());
        let yaml_string = serde_yaml::to_string(&yaml_value).unwrap();
        println!("{}", yaml_string);
        std::fs::write("generated.openapi.yaml", yaml_string).unwrap();
    }

    #[tokio::test]
    #[should_panic]
    async fn test_base_path_routes() {
        let issuance_state = in_memory::issuance_state(Service::default(), Default::default()).await;
        std::env::set_var("UNICORE__BASE_PATH", "unicore");
        let router = app(ApplicationState {
            issuance_state: Some(issuance_state),
            ..Default::default()
        });

        let _ = router.route("/auth/token", post(handler));
    }
}
