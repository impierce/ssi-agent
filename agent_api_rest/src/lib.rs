mod credential_issuer;
mod credentials;
mod offers;

use agent_issuance::state::ApplicationState;
use agent_shared::{config, ConfigError};
use axum::{
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
use credentials::credentials;
use offers::offers;

pub const SERVER_CONFIG_ID: &str = "SERVER-CONFIG-001";

pub fn app(app_state: ApplicationState) -> Router {
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
        .route(&path("/v1/offers"), post(offers))
        // OpenID4VCI Pre-Authorized Code Flow
        .route(
            &path("/.well-known/oauth-authorization-server"),
            get(oauth_authorization_server),
        )
        .route("/.well-known/openid-credential-issuer", get(openid_credential_issuer))
        .route("/auth/token", post(token))
        .route("/openid4vci/credential", post(credential))
        .with_state(app_state)
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
