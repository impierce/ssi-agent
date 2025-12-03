use agent_library::template::event::Authorization;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use jsonwebtoken::Algorithm;
use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
};
use oid4vci::proof::{KeyProofMetadata, ProofType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

use crate::server_config::command::ServerConfigCommand;
use crate::server_config::error::ServerConfigError;
use crate::server_config::event::ServerConfigEvent;
use crate::services::IssuanceServices;

fn into_credential_configurations_supported(
    credential_configurations: &HashMap<String, (String, CredentialConfigurationsSupportedObject, Authorization)>,
) -> HashMap<String, CredentialConfigurationsSupportedObject> {
    credential_configurations
        .iter()
        .map(
            |(_template_id, (credential_configuration_id, credential_configuration, _authorization))| {
                (credential_configuration_id.clone(), credential_configuration.clone())
            },
        )
        .collect()
}

fn into_credential_signing_alg_values_supported(signing_algorithms_supported: &[Algorithm]) -> Vec<String> {
    signing_algorithms_supported
        .iter()
        .map(|algorithm| match algorithm {
            jsonwebtoken::Algorithm::EdDSA => "EdDSA".to_string(),
            jsonwebtoken::Algorithm::ES256 => "ES256".to_string(),
            _ => unimplemented!("Unsupported algorithm: {:?}", algorithm),
        })
        .collect()
}

fn into_proof_types_supported(signing_algorithms_supported: &[Algorithm]) -> HashMap<ProofType, KeyProofMetadata> {
    HashMap::from_iter([(
        ProofType::Jwt,
        KeyProofMetadata {
            proof_signing_alg_values_supported: signing_algorithms_supported.to_vec(),
        },
    )])
}

/// An aggregate that holds the configuration of the server.
#[derive(Clone, Default, Deserialize, Serialize, Debug)]
pub struct ServerConfig {
    pub authorization_server_metadata: AuthorizationServerMetadata,
    pub credential_issuer_metadata: CredentialIssuerMetadata,
    pub credential_configurations: HashMap<String, (String, CredentialConfigurationsSupportedObject, Authorization)>,
    pub cryptographic_binding_methods_supported: Vec<String>,
    pub signing_algorithms_supported: Vec<Algorithm>,
}

#[async_trait]
impl Aggregate for ServerConfig {
    type Command = ServerConfigCommand;
    type Event = ServerConfigEvent;
    type Error = ServerConfigError;
    type Services = Arc<IssuanceServices>;

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
                cryptographic_binding_methods_supported,
                signing_algorithms_supported,
            } => Ok(vec![ServerMetadataInitialized {
                authorization_server_metadata,
                credential_issuer_metadata,
                cryptographic_binding_methods_supported,
                signing_algorithms_supported,
            }]),
            UpdateIssuerUrl { url } => {
                let mut authorization_server_metadata = self.authorization_server_metadata.clone();
                authorization_server_metadata.issuer = url.clone();

                let mut credential_issuer_metadata = self.credential_issuer_metadata.clone();
                credential_issuer_metadata.credential_issuer = url;

                Ok(vec![IssuerUrlUpdated {
                    authorization_server_metadata: Box::new(authorization_server_metadata),
                    credential_issuer_metadata: Box::new(credential_issuer_metadata),
                }])
            }
            UpdateIssuerDisplay { display } => {
                let mut credential_issuer_metadata = self.credential_issuer_metadata.clone();
                credential_issuer_metadata.display = display;

                Ok(vec![IssuerDisplayUpdated {
                    credential_issuer_metadata: Box::new(credential_issuer_metadata),
                }])
            }
            UpdateCryptographicBindingMethods {
                cryptographic_binding_methods_supported,
            } => {
                let mut credential_configurations = self.credential_configurations.clone();

                for (_template_id, (_credential_configuration_id, credential_configuration, _authorization)) in
                    credential_configurations.iter_mut()
                {
                    credential_configuration.cryptographic_binding_methods_supported =
                        cryptographic_binding_methods_supported.clone();
                }

                let mut credential_issuer_metadata = Box::new(self.credential_issuer_metadata.clone());
                credential_issuer_metadata.credential_configurations_supported =
                    into_credential_configurations_supported(&credential_configurations);

                Ok(vec![CryptographicBindingMethodsUpdated {
                    cryptographic_binding_methods_supported,
                    credential_issuer_metadata,
                    credential_configurations,
                }])
            }
            UpdateSigningAlgorithms {
                signing_algorithms_supported,
            } => {
                let mut credential_configurations = self.credential_configurations.clone();

                for (_template_id, (_credential_configuration_id, credential_configuration, _authorization)) in
                    credential_configurations.iter_mut()
                {
                    credential_configuration.credential_signing_alg_values_supported =
                        into_credential_signing_alg_values_supported(&signing_algorithms_supported);
                    credential_configuration.proof_types_supported =
                        into_proof_types_supported(&signing_algorithms_supported);
                }

                let mut credential_issuer_metadata = Box::new(self.credential_issuer_metadata.clone());
                credential_issuer_metadata.credential_configurations_supported =
                    into_credential_configurations_supported(&credential_configurations);

                Ok(vec![SigningAlgorithmsUpdated {
                    signing_algorithms_supported,
                    credential_issuer_metadata,
                    credential_configurations,
                }])
            }

            CreateCredentialConfiguration {
                template_id,
                credential_configuration_id,
                credential_format_with_parameters,
                display,
                claims,
                authorization,
            } => {
                let proof_types_supported = into_proof_types_supported(&self.signing_algorithms_supported);

                let credential_configuration_object = CredentialConfigurationsSupportedObject {
                    credential_format: credential_format_with_parameters,
                    cryptographic_binding_methods_supported: self.cryptographic_binding_methods_supported.clone(),
                    credential_signing_alg_values_supported: into_credential_signing_alg_values_supported(
                        &self.signing_algorithms_supported,
                    ),
                    proof_types_supported,
                    display,
                    claims,
                    ..Default::default()
                };

                let mut credential_configurations = self.credential_configurations.clone();
                if let Some((existing_credential_configuration_id, existing_credential_configuration, _authorization)) =
                    credential_configurations.get_mut(&template_id)
                {
                    *existing_credential_configuration_id = credential_configuration_id.clone();
                    *existing_credential_configuration = credential_configuration_object;
                } else {
                    credential_configurations.insert(
                        template_id,
                        (
                            credential_configuration_id.clone(),
                            credential_configuration_object,
                            authorization,
                        ),
                    );
                }

                let mut credential_issuer_metadata = Box::new(self.credential_issuer_metadata.clone());
                credential_issuer_metadata.credential_configurations_supported =
                    into_credential_configurations_supported(&credential_configurations);

                Ok(vec![CredentialConfigurationCreated {
                    credential_configuration_id,
                    credential_issuer_metadata,
                    credential_configurations,
                }])
            }
            UpdateCredentialConfigurationId {
                template_id,
                credential_configuration_id,
            } => {
                let mut credential_configurations = self.credential_configurations.clone();
                if let Some((
                    existing_credential_configuration_id,
                    _existing_credential_configuration,
                    _authorization,
                )) = credential_configurations.get_mut(&template_id)
                {
                    *existing_credential_configuration_id = credential_configuration_id;
                }

                let mut credential_issuer_metadata = Box::new(self.credential_issuer_metadata.clone());
                credential_issuer_metadata.credential_configurations_supported =
                    into_credential_configurations_supported(&credential_configurations);

                Ok(vec![CredentialConfigurationIdUpdated {
                    credential_issuer_metadata,
                    credential_configurations,
                }])
            }
            UpdateCredentialConfigurationDisplay { template_id, display } => {
                let mut credential_configurations = self.credential_configurations.clone();
                if let Some((_, existing_credential_configuration, _authorization)) =
                    credential_configurations.get_mut(&template_id)
                {
                    existing_credential_configuration.display = vec![display];
                }

                let mut credential_issuer_metadata = Box::new(self.credential_issuer_metadata.clone());
                credential_issuer_metadata.credential_configurations_supported =
                    into_credential_configurations_supported(&credential_configurations);

                Ok(vec![CredentialConfigurationDisplayUpdated {
                    credential_issuer_metadata,
                    credential_configurations,
                }])
            }
            UpdateCredentialConfigurationAuthorization {
                template_id,
                authorization,
            } => {
                let mut credential_configurations = self.credential_configurations.clone();
                if let Some((_, _, existing_authorization)) = credential_configurations.get_mut(&template_id) {
                    *existing_authorization = authorization;
                }

                let mut credential_issuer_metadata = Box::new(self.credential_issuer_metadata.clone());
                credential_issuer_metadata.credential_configurations_supported =
                    into_credential_configurations_supported(&credential_configurations);

                Ok(vec![CredentialConfigurationAuthorizationUpdated {
                    credential_issuer_metadata,
                    credential_configurations,
                }])
            }
            RemoveCredentialConfiguration { template_id } => {
                let mut credential_configurations = self.credential_configurations.clone();
                credential_configurations.remove(&template_id);

                let mut credential_issuer_metadata = Box::new(self.credential_issuer_metadata.clone());
                credential_issuer_metadata.credential_configurations_supported =
                    into_credential_configurations_supported(&credential_configurations);

                Ok(vec![CredentialConfigurationRemoved {
                    credential_issuer_metadata,
                    credential_configurations,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use ServerConfigEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            ServerMetadataInitialized {
                authorization_server_metadata,
                credential_issuer_metadata,
                cryptographic_binding_methods_supported,
                signing_algorithms_supported,
            } => {
                self.authorization_server_metadata = *authorization_server_metadata;
                self.credential_issuer_metadata = *credential_issuer_metadata;
                self.cryptographic_binding_methods_supported = cryptographic_binding_methods_supported;
                self.signing_algorithms_supported = signing_algorithms_supported;
            }
            IssuerUrlUpdated {
                authorization_server_metadata,
                credential_issuer_metadata,
            } => {
                self.authorization_server_metadata = *authorization_server_metadata;
                self.credential_issuer_metadata = *credential_issuer_metadata;
            }
            IssuerDisplayUpdated {
                credential_issuer_metadata,
            } => {
                self.credential_issuer_metadata = *credential_issuer_metadata;
            }
            CryptographicBindingMethodsUpdated {
                cryptographic_binding_methods_supported,
                credential_issuer_metadata,
                credential_configurations,
            } => {
                self.cryptographic_binding_methods_supported = cryptographic_binding_methods_supported;
                self.credential_issuer_metadata = *credential_issuer_metadata;
                self.credential_configurations = credential_configurations;
            }
            SigningAlgorithmsUpdated {
                signing_algorithms_supported,
                credential_issuer_metadata,
                credential_configurations,
            } => {
                self.signing_algorithms_supported = signing_algorithms_supported;
                self.credential_issuer_metadata = *credential_issuer_metadata;
                self.credential_configurations = credential_configurations;
            }

            CredentialConfigurationCreated {
                credential_configuration_id: _,
                credential_issuer_metadata,
                credential_configurations,
            } => {
                self.credential_issuer_metadata = *credential_issuer_metadata;
                self.credential_configurations = credential_configurations;
            }
            CredentialConfigurationIdUpdated {
                credential_issuer_metadata,
                credential_configurations,
            } => {
                self.credential_issuer_metadata = *credential_issuer_metadata;
                self.credential_configurations = credential_configurations;
            }
            CredentialConfigurationDisplayUpdated {
                credential_issuer_metadata,
                credential_configurations,
            } => {
                self.credential_issuer_metadata = *credential_issuer_metadata;
                self.credential_configurations = credential_configurations;
            }
            CredentialConfigurationAuthorizationUpdated {
                credential_issuer_metadata,
                credential_configurations,
            } => {
                self.credential_issuer_metadata = *credential_issuer_metadata;
                self.credential_configurations = credential_configurations;
            }
            CredentialConfigurationRemoved {
                credential_issuer_metadata,
                credential_configurations,
            } => {
                self.credential_issuer_metadata = *credential_issuer_metadata;
                self.credential_configurations = credential_configurations;
            }
        }
    }
}

#[cfg(test)]
pub mod server_config_tests {
    use super::test_utils::*;
    use super::*;
    use crate::server_config::aggregate::ServerConfig;
    use crate::server_config::event::ServerConfigEvent;
    use agent_secret_manager::service::Service;
    use cqrs_es::test::TestFramework;
    use rstest::*;

    type ServerConfigTestFramework = TestFramework<ServerConfig>;

    #[rstest]
    fn test_load_server_metadata(
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        cryptographic_binding_methods_supported: Vec<String>,
        signing_algorithms_supported: Vec<Algorithm>,
    ) {
        ServerConfigTestFramework::with(Service::default())
            .given_no_previous_events()
            .when(ServerConfigCommand::InitializeServerMetadata {
                authorization_server_metadata: authorization_server_metadata.clone(),
                credential_issuer_metadata: credential_issuer_metadata.clone(),
                cryptographic_binding_methods_supported: cryptographic_binding_methods_supported.clone(),
                signing_algorithms_supported: signing_algorithms_supported.clone(),
            })
            .then_expect_events(vec![ServerConfigEvent::ServerMetadataInitialized {
                authorization_server_metadata,
                credential_issuer_metadata,
                cryptographic_binding_methods_supported,
                signing_algorithms_supported,
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
    pub fn static_issuer_url() -> url::Url {
        "https://my-domain.example.org/".parse().unwrap()
    }

    #[fixture]
    pub fn credential_configuration_id() -> String {
        "001".to_string()
    }

    #[fixture]
    pub fn cryptographic_binding_methods_supported() -> Vec<String> {
        vec!["did:jwk".to_string(), "did:key".to_string()]
    }

    #[fixture]
    pub fn signing_algorithms_supported() -> Vec<Algorithm> {
        vec![Algorithm::ES256, Algorithm::EdDSA]
    }

    #[fixture]
    pub fn credential_configurations(
        credential_configuration_id: String,
    ) -> HashMap<String, (bool, CredentialConfigurationsSupportedObject, Authorization)> {
        HashMap::from_iter(vec![(
            credential_configuration_id,
            (
                false,
                W3C_VC_CREDENTIAL_CONFIGURATION.clone(),
                Authorization {
                    pre_authorized: true,
                    tx_code_constraints: None,
                },
            ),
        )])
    }

    #[fixture]
    pub fn credential_configurations_supported(
        credential_configurations: HashMap<String, (bool, CredentialConfigurationsSupportedObject, Authorization)>,
    ) -> HashMap<String, CredentialConfigurationsSupportedObject> {
        credential_configurations
            .into_iter()
            .map(
                |(credential_configuration_id, (_provisioned, credential_configuration, _authorization_grant))| {
                    (credential_configuration_id, credential_configuration)
                },
            )
            .collect()
    }

    #[fixture]
    pub fn authorization_server_metadata(static_issuer_url: Url) -> Box<AuthorizationServerMetadata> {
        Box::new(AuthorizationServerMetadata {
            issuer: static_issuer_url.clone(),
            token_endpoint: Some(static_issuer_url.join("token").unwrap()),
            ..Default::default()
        })
    }

    #[fixture]
    pub fn credential_issuer_metadata(static_issuer_url: Url) -> Box<CredentialIssuerMetadata> {
        Box::new(CredentialIssuerMetadata {
            credential_issuer: static_issuer_url.clone(),
            credential_endpoint: static_issuer_url.join("credential").unwrap(),
            ..Default::default()
        })
    }

    #[fixture]
    pub fn credential_issuer_metadata_with_credential_configuration(
        mut credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject>,
    ) -> Box<CredentialIssuerMetadata> {
        credential_issuer_metadata.credential_configurations_supported = credential_configurations_supported;
        credential_issuer_metadata
    }
}
