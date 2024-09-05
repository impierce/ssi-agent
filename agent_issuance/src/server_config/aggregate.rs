use agent_shared::config::config;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use jsonwebtoken::Algorithm;
use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
};
use oid4vci::proof::KeyProofMetadata;
use oid4vci::ProofType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;

use crate::server_config::command::ServerConfigCommand;
use crate::server_config::error::ServerConfigError;
use crate::server_config::event::ServerConfigEvent;

/// An aggregate that holds the configuration of the server.
#[derive(Clone, Default, Deserialize, Serialize, Debug)]
pub struct ServerConfig {
    authorization_server_metadata: AuthorizationServerMetadata,
    credential_issuer_metadata: CredentialIssuerMetadata,
}

#[async_trait]
impl Aggregate for ServerConfig {
    type Command = ServerConfigCommand;
    type Event = ServerConfigEvent;
    type Error = ServerConfigError;
    type Services = ();

    fn aggregate_type() -> String {
        "server_config".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use ServerConfigCommand::*;
        use ServerConfigEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            InitializeServerMetadata {
                authorization_server_metadata,
                credential_issuer_metadata,
            } => Ok(vec![ServerMetadataInitialized {
                authorization_server_metadata,
                credential_issuer_metadata,
            }]),

            AddCredentialConfiguration {
                credential_configuration,
            } => {
                let mut cryptographic_binding_methods_supported: Vec<_> = config()
                    .did_methods
                    .iter()
                    .filter(|&(_, options)| options.enabled)
                    .map(|(did_method, _)| did_method.to_string())
                    .collect();

                cryptographic_binding_methods_supported.sort();

                let signing_algorithms_supported: Vec<Algorithm> = config()
                    .signing_algorithms_supported
                    .iter()
                    .filter(|(_, options)| options.enabled)
                    .map(|(alg, _)| *alg)
                    .collect();

                let proof_types_supported = HashMap::from_iter([(
                    ProofType::Jwt,
                    KeyProofMetadata {
                        proof_signing_alg_values_supported: signing_algorithms_supported.clone(),
                    },
                )]);

                let credential_configuration_object = CredentialConfigurationsSupportedObject {
                    credential_format: credential_configuration.credential_format_with_parameters,
                    cryptographic_binding_methods_supported,
                    credential_signing_alg_values_supported: signing_algorithms_supported
                        .into_iter()
                        .map(|algorithm| match algorithm {
                            jsonwebtoken::Algorithm::EdDSA => "EdDSA".to_string(),
                            jsonwebtoken::Algorithm::ES256 => "ES256".to_string(),
                            _ => unimplemented!("Unsupported algorithm: {:?}", algorithm),
                        })
                        .collect(),
                    proof_types_supported,
                    display: credential_configuration.display,
                    ..Default::default()
                };

                let credential_configurations = HashMap::from_iter([(
                    credential_configuration.credential_configuration_id,
                    credential_configuration_object,
                )]);
                // TODO: Uncomment this once we support Batch credentials.
                // let mut credential_configurations = self
                //     .credential_issuer_metadata
                //     .credential_configurations_supported
                //     .clone();
                // credential_configurations.insert(credential_configuration_id, credential_configuration);

                Ok(vec![CredentialConfigurationAdded {
                    credential_configurations,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use ServerConfigEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            ServerMetadataInitialized {
                authorization_server_metadata,
                credential_issuer_metadata,
            } => {
                self.authorization_server_metadata = *authorization_server_metadata;
                self.credential_issuer_metadata = credential_issuer_metadata;
            }
            CredentialConfigurationAdded {
                credential_configurations,
            } => self.credential_issuer_metadata.credential_configurations_supported = credential_configurations,
        }
    }
}

#[cfg(test)]
pub mod server_config_tests {
    use std::collections::HashMap;

    use super::*;

    use agent_shared::config::CredentialConfiguration;
    use lazy_static::lazy_static;
    use oid4vci::credential_format_profiles::w3c_verifiable_credentials::jwt_vc_json::JwtVcJson;
    use oid4vci::credential_format_profiles::{w3c_verifiable_credentials, CredentialFormats, Parameters};
    use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
    use serde_json::json;

    use cqrs_es::test::TestFramework;

    use crate::server_config::aggregate::ServerConfig;
    use crate::server_config::event::ServerConfigEvent;

    type ServerConfigTestFramework = TestFramework<ServerConfig>;

    #[test]
    fn test_load_server_metadata() {
        ServerConfigTestFramework::with(())
            .given_no_previous_events()
            .when(ServerConfigCommand::InitializeServerMetadata {
                authorization_server_metadata: AUTHORIZATION_SERVER_METADATA.clone(),
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            })
            .then_expect_events(vec![ServerConfigEvent::ServerMetadataInitialized {
                authorization_server_metadata: AUTHORIZATION_SERVER_METADATA.clone(),
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            }]);
    }
    #[test]
    fn test_create_credentials_supported() {
        ServerConfigTestFramework::with(())
            .given(vec![ServerConfigEvent::ServerMetadataInitialized {
                authorization_server_metadata: AUTHORIZATION_SERVER_METADATA.clone(),
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            }])
            .when(ServerConfigCommand::AddCredentialConfiguration {
                credential_configuration: CredentialConfiguration {
                    credential_configuration_id: "0".to_string(),
                    credential_format_with_parameters: CredentialFormats::JwtVcJson(Parameters::<JwtVcJson> {
                        parameters: w3c_verifiable_credentials::jwt_vc_json::JwtVcJsonParameters {
                            credential_definition: w3c_verifiable_credentials::jwt_vc_json::CredentialDefinition {
                                type_: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
                                credential_subject: Default::default(),
                            },
                            order: None,
                        },
                    }),
                    display: vec![],
                },
            })
            .then_expect_events(vec![ServerConfigEvent::CredentialConfigurationAdded {
                credential_configurations: CREDENTIAL_CONFIGURATIONS_SUPPORTED.clone(),
            }]);
    }

    lazy_static! {
        static ref BASE_URL: url::Url = "https://example.com/".parse().unwrap();
        static ref CREDENTIAL_CONFIGURATIONS_SUPPORTED: HashMap<String, CredentialConfigurationsSupportedObject> =
            vec![(
                "0".to_string(),
                serde_json::from_value(json!({
                    "format": "jwt_vc_json",
                    "cryptographic_binding_methods_supported": [
                        "did:iota:rms",
                        "did:jwk",
                        "did:key",
                    ],
                    "credential_signing_alg_values_supported": [
                        "EdDSA"
                    ],
                    "proof_types_supported": {
                        "jwt": {
                            "proof_signing_alg_values_supported": [
                                "EdDSA"
                            ]
                        }
                    },
                    "credential_definition":{
                        "type": [
                            "VerifiableCredential",
                            "OpenBadgeCredential"
                        ]
                    }
                }
                ))
                .unwrap()
            )]
            .into_iter()
            .collect();
        pub static ref AUTHORIZATION_SERVER_METADATA: Box<AuthorizationServerMetadata> =
            Box::new(AuthorizationServerMetadata {
                issuer: BASE_URL.clone(),
                token_endpoint: Some(BASE_URL.join("token").unwrap()),
                ..Default::default()
            });
        pub static ref CREDENTIAL_ISSUER_METADATA: CredentialIssuerMetadata = CredentialIssuerMetadata {
            credential_issuer: BASE_URL.clone(),
            credential_endpoint: BASE_URL.join("credential").unwrap(),
            batch_credential_endpoint: Some(BASE_URL.join("batch_credential").unwrap()),
            credential_configurations_supported: CREDENTIAL_CONFIGURATIONS_SUPPORTED.clone(),
            ..Default::default()
        };
    }
}
