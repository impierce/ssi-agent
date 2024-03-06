mod credential_issuer;
mod credentials;
mod offers;

use agent_issuance::state::ApplicationState;
use agent_shared::{config, ConfigError};
use axum::{
    body::Bytes,
    extract::MatchedPath,
    http::Request,
    response::Response,
    routing::{get, post},
    Router,
};
use credential_issuer::{
    credential::credential,
    token::token,
    well_known::{
        oauth_authorization_server::oauth_authorization_server, openid_credential_issuer::openid_credential_issuer,
    },
};
use credentials::{credentials, get_credentials};
use offers::offers;
use tower_http::trace::TraceLayer;
use tracing::{info_span, Span};

#[macro_export]
macro_rules! log_error_response {
    (($status_code:expr, $message:literal)) => {{
        tracing::error!("Returning {}: {}", $status_code, $message);
        $status_code.into_response()
    }};
    ($status_code:expr) => {{
        tracing::error!("Returning {}", $status_code);
        $status_code.into_response()
    }};
}

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
        // Agent Preparations
        .route(&path("/v1/credentials"), post(credentials))
        .route(&path("/v1/credentials/:credential_id"), get(get_credentials))
        .route(&path("/v1/offers"), post(offers))
        // OpenID4VCI Pre-Authorized Code Flow
        .route(
            &path("/.well-known/oauth-authorization-server"),
            get(oauth_authorization_server),
        )
        .route("/.well-known/openid-credential-issuer", get(openid_credential_issuer))
        .route("/auth/token", post(token))
        .route("/openid4vci/credential", post(credential))
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
    config!("base_path").map(|mut base_path| {
        if base_path.starts_with('/') {
            base_path.remove(0);
        }

        if base_path.ends_with('/') {
            base_path.pop();
        }

        if base_path.is_empty() {
            panic!("AGENT_APPLICATION_BASE_PATH can't be empty, remove or set path");
        }

        tracing::info!("Base path: {:?}", base_path);

        base_path
    })
}

#[cfg(test)]
mod tests {
    use agent_store::in_memory;
    use axum::routing::post;
    use oid4vci::credential_issuer::{
        credential_issuer_metadata::CredentialIssuerMetadata, credentials_supported::CredentialsSupportedObject,
    };
    use serde_json::json;

    use crate::app;

    pub const SUBJECT_ID: &str = "00000000-0000-0000-0000-000000000000";

    lazy_static::lazy_static! {
        pub static ref BASE_URL: url::Url = url::Url::parse("https://example.com").unwrap();
        static ref CREDENTIALS_SUPPORTED: Vec<CredentialsSupportedObject> = vec![serde_json::from_value(json!({
            "format": "jwt_vc_json",
            "cryptographic_binding_methods_supported": [
                "did:key",
            ],
            "cryptographic_suites_supported": [
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
        .unwrap()];
        pub static ref CREDENTIAL_ISSUER_METADATA: CredentialIssuerMetadata = CredentialIssuerMetadata {
            credential_issuer: BASE_URL.clone(),
            authorization_server: None,
            credential_endpoint: BASE_URL.join("credential").unwrap(),
            deferred_credential_endpoint: None,
            batch_credential_endpoint: Some(BASE_URL.join("batch_credential").unwrap()),
            credentials_supported: CREDENTIALS_SUPPORTED.clone(),
            display: None,
        };
    }

    async fn handler() {}

    #[tokio::test]
    #[should_panic]
    async fn test_base_path_routes() {
        let state = in_memory::application_state().await;

        std::env::set_var("AGENT_APPLICATION_BASE_PATH", "unicore");
        let router = app(state);

        let _ = router.route("/auth/token", post(handler));
    }
}
