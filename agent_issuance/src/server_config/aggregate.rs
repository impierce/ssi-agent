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
                self.credential_issuer_metadata = *credential_issuer_metadata;
            }
            CredentialConfigurationAdded {
                credential_configurations,
            } => self.credential_issuer_metadata.credential_configurations_supported = credential_configurations,
        }
    }
}

#[cfg(test)]
pub mod server_config_tests {
    use super::test_utils::*;
    use super::*;
    use crate::server_config::aggregate::ServerConfig;
    use crate::server_config::event::ServerConfigEvent;
    use agent_shared::config::CredentialConfiguration;
    use cqrs_es::test::TestFramework;
    use oid4vci::credential_format_profiles::w3c_verifiable_credentials::jwt_vc_json::JwtVcJson;
    use oid4vci::credential_format_profiles::{w3c_verifiable_credentials, CredentialFormats, Parameters};
    use rstest::*;
    use serde_json::json;

    type ServerConfigTestFramework = TestFramework<ServerConfig>;

    #[rstest]
    fn test_load_server_metadata(
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
    ) {
        ServerConfigTestFramework::with(())
            .given_no_previous_events()
            .when(ServerConfigCommand::InitializeServerMetadata {
                authorization_server_metadata: authorization_server_metadata.clone(),
                credential_issuer_metadata: credential_issuer_metadata.clone(),
            })
            .then_expect_events(vec![ServerConfigEvent::ServerMetadataInitialized {
                authorization_server_metadata,
                credential_issuer_metadata,
            }]);
    }
    #[rstest]
    fn test_create_credentials_supported(
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
    ) {
        ServerConfigTestFramework::with(())
            .given(vec![ServerConfigEvent::ServerMetadataInitialized {
                authorization_server_metadata,
                credential_issuer_metadata: credential_issuer_metadata.clone(),
            }])
            .when(ServerConfigCommand::AddCredentialConfiguration {
                credential_configuration: CredentialConfiguration {
                    credential_configuration_id: "badge".to_string(),
                    credential_format_with_parameters: CredentialFormats::JwtVcJson(Parameters::<JwtVcJson> {
                        parameters: w3c_verifiable_credentials::jwt_vc_json::JwtVcJsonParameters {
                            credential_definition: w3c_verifiable_credentials::jwt_vc_json::CredentialDefinition {
                                type_: vec!["VerifiableCredential".to_string()],
                                credential_subject: Default::default(),
                            },
                            order: None,
                        },
                    }),
                    display: vec![json!({
                        "name": "Verifiable Credential",
                        "locale": "en",
                        "logo": {
                            "uri": "https://impierce.com/images/logo-blue.png",
                            "alt_text": "UniCore Logo"
                        }
                    })],
                },
            })
            .then_expect_events(vec![ServerConfigEvent::CredentialConfigurationAdded {
                credential_configurations: credential_issuer_metadata.credential_configurations_supported,
            }]);
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use crate::credential::aggregate::test_utils::W3C_VC_CREDENTIAL_CONFIGURATION;
    use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
    use rstest::*;
    use url::Url;

    #[fixture]
    #[once]
    pub fn static_issuer_url() -> url::Url {
        "https://example.com/".parse().unwrap()
    }

    #[fixture]
    pub fn credential_configurations_supported() -> HashMap<String, CredentialConfigurationsSupportedObject> {
        HashMap::from_iter(vec![("badge".to_string(), W3C_VC_CREDENTIAL_CONFIGURATION.clone())])
    }

    #[fixture]
    pub fn authorization_server_metadata(static_issuer_url: &Url) -> Box<AuthorizationServerMetadata> {
        Box::new(AuthorizationServerMetadata {
            issuer: static_issuer_url.clone(),
            token_endpoint: Some(static_issuer_url.join("token").unwrap()),
            ..Default::default()
        })
    }

    #[fixture]
    pub fn credential_issuer_metadata(
        static_issuer_url: &Url,
        credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject>,
    ) -> Box<CredentialIssuerMetadata> {
        Box::new(CredentialIssuerMetadata {
            credential_issuer: static_issuer_url.clone(),
            credential_endpoint: static_issuer_url.join("credential").unwrap(),
            batch_credential_endpoint: Some(static_issuer_url.join("batch_credential").unwrap()),
            credential_configurations_supported,
            ..Default::default()
        })
    }
}
