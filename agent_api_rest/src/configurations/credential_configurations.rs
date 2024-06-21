use agent_issuance::{
    server_config::{command::ServerConfigCommand, queries::ServerConfigView},
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use oid4vci::{
    credential_format_profiles::{CredentialFormats, WithParameters},
    credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialConfigurationsEndpointRequest {
    pub credential_configuration_id: String,
    #[serde(flatten)]
    pub credential_format_with_parameters: CredentialFormats<WithParameters>,
    #[serde(default)]
    pub display: Vec<serde_json::Value>,
}

#[axum_macros::debug_handler]
pub(crate) async fn credential_configurations(
    State(state): State<IssuanceState>,
    Json(payload): Json<Value>,
) -> Response {
    info!("Request Body: {}", payload);

    let Ok(CredentialConfigurationsEndpointRequest {
        credential_configuration_id,
        credential_format_with_parameters,
        display,
    }) = serde_json::from_value(payload)
    else {
        return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
    };

    let command = ServerConfigCommand::AddCredentialConfiguration {
        credential_configuration_id,
        credential_format_with_parameters,
        display,
    };

    if command_handler(SERVER_CONFIG_ID, &state.command.server_config, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await {
        Ok(Some(ServerConfigView {
            credential_issuer_metadata:
                Some(CredentialIssuerMetadata {
                    credential_configurations_supported,
                    ..
                }),
            ..
        })) => (StatusCode::OK, Json(credential_configurations_supported)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
pub mod tests {
    use std::collections::HashMap;

    use crate::{
        app,
        tests::{BASE_URL, CREDENTIAL_CONFIGURATION_ID},
    };

    use super::*;
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_shared::metadata::{load_metadata, set_metadata_configuration};
    use agent_store::in_memory;
    use agent_verification::services::test_utils::test_verification_services;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use jsonwebtoken::Algorithm;
    use oid4vci::{
        credential_format_profiles::{w3c_verifiable_credentials::jwt_vc_json::CredentialDefinition, Parameters},
        credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject,
        proof::KeyProofMetadata,
        ProofType,
    };
    use serde_json::json;
    use tower::Service;

    pub async fn credential_configurations(app: &mut Router) {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/configurations/credential_configurations")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "credentialConfigurationId": CREDENTIAL_CONFIGURATION_ID,
                            "format": "jwt_vc_json",
                            "credential_definition": {
                                "type": [
                                    "VerifiableCredential"
                                ]
                            },
                            "display": [{
                                "name": "Badge",
                                "logo": {
                                    "url": "https://example.com/logo.png",
                               }
                            }]
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject> =
            serde_json::from_slice(&body).unwrap();

        assert_eq!(
            credential_configurations_supported,
            HashMap::from_iter([(
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
                    cryptographic_binding_methods_supported: vec![
                        "did:key".to_string(),
                        "did:key".to_string(),
                        "did:iota:rms".to_string(),
                        "did:jwk".to_string(),
                    ],
                    credential_signing_alg_values_supported: vec!["EdDSA".to_string()],
                    proof_types_supported: HashMap::from_iter([(
                        ProofType::Jwt,
                        KeyProofMetadata {
                            proof_signing_alg_values_supported: vec![Algorithm::EdDSA],
                        },
                    )]),
                    display: vec![json!({
                        "name": "Badge",
                        "logo": {
                            "url": "https://example.com/logo.png"
                        }
                    })],
                    ..Default::default()
                }
            )])
        );
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_credential_configurations() {
        set_metadata_configuration("did:key");

        let issuance_state = in_memory::issuance_state(Default::default()).await;

        let verification_state = in_memory::verification_state(test_verification_services(), Default::default()).await;

        initialize(&issuance_state, startup_commands(BASE_URL.clone(), &load_metadata())).await;

        let mut app = app((issuance_state, verification_state));

        credential_configurations(&mut app).await;
    }
}
