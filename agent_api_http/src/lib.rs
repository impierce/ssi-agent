pub mod public;
pub mod v0;

pub mod error;
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
use axum::Router;
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use std::sync::Arc;

pub const API_VERSION: &str = "/v0";

pub const DOCUMENTATION_URL: &str = "https://beta.docs.impierce.com/unicore/";

#[derive(Default)]
pub struct ApplicationState {
    pub identity_state: Option<Arc<IdentityState>>,
    pub library_state: Option<Arc<LibraryState>>,
    pub authorization_state: Option<Arc<AuthorizationState>>,
    pub issuance_state: Option<Arc<IssuanceState>>,
    pub holder_state: Option<Arc<HolderState>>,
    pub verification_state: Option<Arc<VerificationState>>,
}

pub fn app(
    ApplicationState {
        identity_state,
        library_state,
        authorization_state,
        issuance_state,
        holder_state,
        verification_state,
    }: ApplicationState,
) -> Router {
    let app = Router::new()
        .merge(identity_state.map(v0::identity::router).unwrap_or_default())
        .merge(library_state.map(v0::library::router).unwrap_or_default())
        .merge(
            authorization_state
                // The `IssuanceState` is cloned here to ensure that the authorization router can access it. This is
                // necessary since for the Pre-Authorized Code flow, the Token Endpoint requires a shared state with
                // the Issuance Bounded Context.
                .zip(issuance_state.clone())
                .map(v0::authorization::router)
                .unwrap_or_default(),
        )
        .merge(issuance_state.map(v0::issuance::router).unwrap_or_default())
        .merge(holder_state.map(v0::holder::router).unwrap_or_default())
        .merge(verification_state.map(v0::verification::router).unwrap_or_default())
        .merge(public::router())
        // Include trace context in response headers
        .layer(OtelInResponseLayer::default())
        // Start traces on incoming requests
        .layer(OtelAxumLayer::default());

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
                    },
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
}
