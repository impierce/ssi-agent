mod issuance;
mod verification;

use agent_issuance::state::IssuanceState;
use agent_shared::{config::config, ConfigError};
use agent_verification::state::VerificationState;
use axum::{
    body::Bytes,
    extract::MatchedPath,
    http::Request,
    response::Response,
    routing::{get, post},
    Router,
};
use issuance::credential_issuer::{
    credential::credential,
    token::token,
    well_known::{
        oauth_authorization_server::oauth_authorization_server, openid_credential_issuer::openid_credential_issuer,
    },
};
use issuance::credentials::{credentials, get_credentials};
use issuance::offers::offers;
use tower_http::trace::TraceLayer;
use tracing::{info_span, Span};
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};
use utoipa_swagger_ui::SwaggerUi;
use verification::{
    authorization_requests::{authorization_requests, get_authorization_requests},
    relying_party::{redirect::redirect, request::request},
};

use crate::issuance::openapi::{IssuanceApi, VerificationApi, WellKnownApi};

pub const API_VERSION: &str = "/v0";

pub type ApplicationState = (IssuanceState, VerificationState);

pub fn app(state: ApplicationState) -> Router {
    let base_path = get_base_path();

    let path = |suffix: &str| -> String {
        if let Ok(base_path) = &base_path {
            format!("/{}{}", base_path, suffix)
        } else {
            suffix.to_string()
        }
    };

    Router::new()
        .merge(Scalar::with_url("/scalar", ApiDoc::openapi()))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .nest(
            &path(API_VERSION),
            Router::new()
                // Agent Issuance Preparations
                .route("/credentials", post(credentials))
                .route("/credentials/:credential_id", get(get_credentials))
                .route("/offers", post(offers))
                // Agent Verification Preparations
                .route("/authorization_requests", post(authorization_requests))
                .route(
                    "/authorization_requests/:authorization_request_id",
                    get(get_authorization_requests),
                ),
        )
        // OpenID4VCI Pre-Authorized Code Flow
        .route(
            &path("/.well-known/oauth-authorization-server"),
            get(oauth_authorization_server),
        )
        .route(
            &path("/.well-known/openid-credential-issuer"),
            get(openid_credential_issuer),
        )
        .route(&path("/auth/token"), post(token))
        .route(&path("/openid4vci/credential"), post(credential))
        // SIOPv2
        .route(&path("/request/:request_id"), get(request))
        .route(&path("/redirect"), post(redirect))
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
        .with_state(state)
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

// #[derive(OpenApi)]
// #[openapi(modifiers(), nest((path = "/v0/todos", api = WellKnownApi)), tags((name = "well-known")))]
// struct ApiDoc;

#[derive(utoipa::OpenApi)]
#[openapi(
        // modifiers(),
        nest(
            (path = "/.well-known", api = WellKnownApi),
            (path = "/v0", api = IssuanceApi),
            (path = "/v0", api = VerificationApi)
        ),
        // paths(
        //     crate::issuance::credential_issuer::CredentialApi
        // ),
        tags(
            // (name = "todo", description = "Todo items management API"),
            (name = "openid4vci", description = "All operations revolved around the OpenID4VCI standard.", external_docs(url = "https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html", description = "OpenID for Verifiable Credential Issuance")),
            (name = "Well-Known", description = "Well-known endpoints provide metadata about the server."),
        )
    )]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use agent_issuance::services::test_utils::test_issuance_services;
    use agent_store::in_memory;
    use agent_verification::services::test_utils::test_verification_services;
    use axum::routing::post;
    use oid4vci::credential_issuer::{
        credential_configurations_supported::CredentialConfigurationsSupportedObject,
        credential_issuer_metadata::CredentialIssuerMetadata,
    };
    use serde_json::json;
    use utoipa::OpenApi;

    use crate::{app, ApiDoc};

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
    async fn openapi() {
        let yaml = serde_yaml::to_string(&ApiDoc::openapi()).unwrap();
        println!("{}", yaml);
        std::fs::write("test.openapi.yaml", yaml).unwrap();
    }

    #[tokio::test]
    #[should_panic]
    async fn test_base_path_routes() {
        let issuance_state = in_memory::issuance_state(test_issuance_services(), Default::default()).await;
        let verification_state = in_memory::verification_state(test_verification_services(), Default::default()).await;
        std::env::set_var("UNICORE__BASE_PATH", "unicore");
        let router = app((issuance_state, verification_state));

        let _ = router.route("/auth/token", post(handler));
    }
}
