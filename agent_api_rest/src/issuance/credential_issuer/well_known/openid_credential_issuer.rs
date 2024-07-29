use agent_issuance::{
    server_config::queries::ServerConfigView,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use agent_shared::handlers::query_handler;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[axum_macros::debug_handler]
pub(crate) async fn openid_credential_issuer(State(state): State<IssuanceState>) -> Response {
    match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await {
        Ok(Some(ServerConfigView {
            credential_issuer_metadata: Some(credential_issuer_metadata),
            ..
        })) => (StatusCode::OK, Json(credential_issuer_metadata)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{app, tests::BASE_URL};

    use super::*;
    use agent_issuance::{
        services::test_utils::test_issuance_services, startup_commands::startup_commands, state::initialize,
    };
    use agent_shared::UrlAppendHelpers;
    use agent_store::in_memory;
    use agent_verification::services::test_utils::test_verification_services;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use jsonwebtoken::Algorithm;
    use oid4vci::{
        credential_format_profiles::{
            w3c_verifiable_credentials::jwt_vc_json::CredentialDefinition, CredentialFormats, Parameters,
        },
        credential_issuer::{
            credential_configurations_supported::CredentialConfigurationsSupportedObject,
            credential_issuer_metadata::CredentialIssuerMetadata,
        },
        proof::KeyProofMetadata,
        ProofType,
    };
    use serde_json::json;
    use tower::Service;

    pub async fn openid_credential_issuer(app: &mut Router) -> CredentialIssuerMetadata {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/.well-known/openid-credential-issuer")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let credential_issuer_metadata: CredentialIssuerMetadata = serde_json::from_slice(&body).unwrap();

        assert_eq!(
            credential_issuer_metadata,
            CredentialIssuerMetadata {
                credential_issuer: BASE_URL.clone(),
                credential_endpoint: BASE_URL.append_path_segment("openid4vci/credential"),
                credential_configurations_supported: vec![(
                    "badge".to_string(),
                    CredentialConfigurationsSupportedObject {
                        credential_format: CredentialFormats::JwtVcJson(Parameters {
                            parameters: (
                                CredentialDefinition {
                                    type_: vec!["VerifiableCredential".to_string()],
                                    credential_subject: Default::default(),
                                },
                                None,
                            )
                                .into(),
                        }),
                        scope: None,
                        cryptographic_binding_methods_supported: vec![
                            "did:iota:rms".to_string(),
                            "did:jwk".to_string(),
                            "did:key".to_string()
                        ],
                        credential_signing_alg_values_supported: vec!["EdDSA".to_string()],
                        proof_types_supported: HashMap::from_iter([(
                            ProofType::Jwt,
                            KeyProofMetadata {
                                proof_signing_alg_values_supported: vec![Algorithm::EdDSA],
                            },
                        )]),
                        display: vec![json!({
                            "name": "Verifiable Credential",
                            "locale": "en",
                            "logo": {
                                "url": "https://impierce.com/images/logo-blue.png",
                                "alt_text": "UniCore Logo"
                            }
                        })],
                    }
                )]
                .into_iter()
                .collect(),
                display: Some(vec![json!({
                    "name": "UniCore",
                    "locale": "en",
                    "logo": {
                        "url": "https://impierce.com/images/favicon/apple-touch-icon.png",
                        "alt_text": "UniCore Logo"
                    }
                })]),
                ..Default::default()
            }
        );

        credential_issuer_metadata
    }

    #[tokio::test]
    async fn test_openid_credential_issuer_endpoint() {
        let issuance_state = in_memory::issuance_state(test_issuance_services(), Default::default()).await;
        let verification_state = in_memory::verification_state(test_verification_services(), Default::default()).await;
        initialize(&issuance_state, startup_commands(BASE_URL.clone())).await;

        let mut app = app((issuance_state, verification_state));

        let _credential_issuer_metadata = openid_credential_issuer(&mut app).await;
    }
}
