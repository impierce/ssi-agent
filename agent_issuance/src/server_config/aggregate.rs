use agent_shared::config::{config, Authorization};
use agent_shared::UrlAppendHelpers as _;
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cqrs_es::Aggregate;
use identity_core::convert::ToJson;
use jsonwebtoken::Algorithm;
use oid4vci::credential_format_profiles::vc_jose_cose::vc_sd_jwt;
use oid4vci::credential_format_profiles::w3c_verifiable_credentials::jwt_vc_json;
use oid4vci::credential_format_profiles::{CredentialFormats, Parameters};
use oid4vci::credential_issuer::credential_configurations_supported::AlgIdentifier;
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
    credential_configurations: &HashMap<String, (bool, CredentialConfigurationsSupportedObject, Authorization)>,
) -> HashMap<String, CredentialConfigurationsSupportedObject> {
    credential_configurations
        .iter()
        .map(
            |(credential_configuration_id, (_provisioned, credential_configuration, _authorization_grant))| {
                (credential_configuration_id.clone(), credential_configuration.clone())
            },
        )
        .collect()
}

fn into_credential_signing_alg_values_supported(signing_algorithms_supported: &[Algorithm]) -> Vec<AlgIdentifier> {
    signing_algorithms_supported
        .iter()
        .filter_map(|algorithm| {
            algorithm
                .to_json_value()
                .ok()
                .and_then(|value| value.as_str().map(|s| AlgIdentifier::String(s.to_string())))
        })
        .collect()
}

fn into_proof_types_supported(signing_algorithms_supported: &[Algorithm]) -> HashMap<ProofType, KeyProofMetadata> {
    HashMap::from_iter([(
        ProofType::Jwt,
        KeyProofMetadata {
            proof_signing_alg_values_supported: into_credential_signing_alg_values_supported(
                signing_algorithms_supported,
            ),
        },
    )])
}

/// An aggregate that holds the configuration of the server.
#[derive(Clone, Default, Deserialize, Serialize, Debug)]
pub struct ServerConfig {
    pub authorization_server_metadata: AuthorizationServerMetadata,
    pub credential_issuer_metadata: CredentialIssuerMetadata,
    pub credential_configurations: HashMap<String, (bool, CredentialConfigurationsSupportedObject, Authorization)>,
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
        use ServerConfigError::*;
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
                if authorization_server_metadata.authorization_endpoint.is_some() {
                    authorization_server_metadata.authorization_endpoint =
                        Some(url.append_path_segment("auth/authorize"));
                }
                if authorization_server_metadata.token_endpoint.is_some() {
                    authorization_server_metadata.token_endpoint = Some(url.append_path_segment("auth/token"));
                }
                if authorization_server_metadata
                    .pushed_authorization_request_endpoint
                    .is_some()
                {
                    authorization_server_metadata.pushed_authorization_request_endpoint =
                        Some(url.append_path_segment("auth/par"));
                }
                if authorization_server_metadata
                    .interactive_authorization_endpoint
                    .is_some()
                {
                    authorization_server_metadata.interactive_authorization_endpoint =
                        Some(url.append_path_segment("auth/par"));
                }

                let mut credential_issuer_metadata = self.credential_issuer_metadata.clone();
                credential_issuer_metadata.credential_issuer = url.clone();
                credential_issuer_metadata.credential_endpoint = url.append_path_segment("openid4vci/credential");
                if credential_issuer_metadata.nonce_endpoint.is_some() {
                    credential_issuer_metadata.nonce_endpoint = Some(url.append_path_segment("openid4vci/nonce"));
                }

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

                for (_credential_configuration_id, (_provisioned, credential_configuration, _authorization_grant)) in
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

                for (_credential_configuration_id, (_provisioned, credential_configuration, _authorization_grant)) in
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
            UpdateCredentialConfiguration {
                credential_configuration,
                provisioned,
            } => {
                let credential_format = match credential_configuration.format.as_str() {
                    "jwt_vc_json" => CredentialFormats::JwtVcJson(Parameters {
                        parameters: (jwt_vc_json::CredentialDefinition {
                            type_: credential_configuration.type_,
                        })
                        .into(),
                    }),
                    "dc+sd-jwt" => {
                        let vct = format!(
                            "{}vct/{}/{version}",
                            config().public_url,
                            URL_SAFE_NO_PAD.encode(&credential_configuration.credential_configuration_id),
                            // TODO: support versioning of VCTs once we support versioning of Templates
                            version = 0
                        );

                        CredentialFormats::DcSdJwt(Parameters {
                            parameters: (vct).into(),
                        })
                    }
                    "vc+sd-jwt" => CredentialFormats::VcSdJwt(Parameters {
                        parameters: (vc_sd_jwt::CredentialDefinition {
                            type_: credential_configuration.type_,
                        })
                        .into(),
                    }),
                    _ => {
                        return Err(UnsupportedCredentialFormatIdentifierError(format!(
                            "{:?}",
                            credential_configuration.format
                        )))
                    }
                };

                let proof_types_supported = into_proof_types_supported(&self.signing_algorithms_supported);

                let credential_configuration_object = CredentialConfigurationsSupportedObject {
                    credential_format,
                    cryptographic_binding_methods_supported: self.cryptographic_binding_methods_supported.clone(),
                    credential_signing_alg_values_supported: into_credential_signing_alg_values_supported(
                        &self.signing_algorithms_supported,
                    ),
                    proof_types_supported,
                    credential_metadata: Some(credential_configuration.credential_metadata),
                    ..Default::default()
                };

                let mut credential_configurations = self.credential_configurations.clone();
                if let Some((existing_provisioned, existing_credential_configuration, existing_authorization_grant)) =
                    credential_configurations.get_mut(&credential_configuration.credential_configuration_id)
                {
                    if !provisioned && *existing_provisioned {
                        return Err(UpdateProvisionedCredentialConfigurationError);
                    }

                    *existing_credential_configuration = credential_configuration_object;
                    *existing_provisioned = provisioned;
                    *existing_authorization_grant = credential_configuration.authorization.clone();
                } else {
                    credential_configurations.insert(
                        credential_configuration.credential_configuration_id.clone(),
                        (
                            provisioned,
                            credential_configuration_object,
                            credential_configuration.authorization.clone(),
                        ),
                    );
                }

                let mut credential_issuer_metadata = Box::new(self.credential_issuer_metadata.clone());
                credential_issuer_metadata.credential_configurations_supported =
                    into_credential_configurations_supported(&credential_configurations);

                Ok(vec![CredentialConfigurationUpdated {
                    credential_configuration_id: credential_configuration.credential_configuration_id,
                    credential_issuer_metadata,
                    credential_configurations,
                }])
            }
            RemoveCredentialConfiguration {
                credential_configuration_id,
                provisioned,
            } => {
                let mut credential_configurations = self.credential_configurations.clone();

                let existing_provisioned = credential_configurations
                    .get(&credential_configuration_id)
                    .map(|(provisioned, _, _)| *provisioned)
                    .unwrap_or(false);

                if !provisioned && existing_provisioned {
                    return Err(RemoveProvisionedCredentialConfigurationError);
                } else {
                    credential_configurations.remove(&credential_configuration_id);
                }

                let mut credential_issuer_metadata = Box::new(self.credential_issuer_metadata.clone());
                credential_issuer_metadata.credential_configurations_supported =
                    into_credential_configurations_supported(&credential_configurations);

                Ok(vec![CredentialConfigurationRemoved {
                    credential_configuration_id,
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
            CredentialConfigurationUpdated {
                credential_configuration_id: _,
                credential_issuer_metadata,
                credential_configurations,
            } => {
                self.credential_issuer_metadata = *credential_issuer_metadata;
                self.credential_configurations = credential_configurations;
            }
            CredentialConfigurationRemoved {
                credential_configuration_id: _,
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
    use agent_shared::config::{Authorization, CredentialConfiguration};
    use cqrs_es::test::TestFramework;
    use oid4vci::credential_issuer::credential_configurations_supported::CredentialMetadata;
    use oid4vci::credential_issuer::credential_configurations_supported::{
        CredentialConfigurationsSupportedDisplay, Logo,
    };
    use rstest::*;

    type ServerConfigTestFramework = TestFramework<ServerConfig>;

    #[rstest]
    async fn test_load_server_metadata(
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        cryptographic_binding_methods_supported: Vec<String>,
        signing_algorithms_supported: Vec<Algorithm>,
    ) {
        ServerConfigTestFramework::with(IssuanceServices::default().await)
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

    #[rstest]
    async fn test_add_credential_configuration(
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        cryptographic_binding_methods_supported: Vec<String>,
        signing_algorithms_supported: Vec<Algorithm>,
        credential_configuration_id: String,
        credential_configurations: HashMap<String, (bool, CredentialConfigurationsSupportedObject, Authorization)>,
        credential_issuer_metadata_with_credential_configuration: Box<CredentialIssuerMetadata>,
    ) {
        ServerConfigTestFramework::with(IssuanceServices::default().await)
            .given(vec![ServerConfigEvent::ServerMetadataInitialized {
                authorization_server_metadata,
                credential_issuer_metadata,
                cryptographic_binding_methods_supported,
                signing_algorithms_supported,
            }])
            .when(ServerConfigCommand::UpdateCredentialConfiguration {
                credential_configuration: CredentialConfiguration {
                    credential_configuration_id: credential_configuration_id.clone(),
                    format: "jwt_vc_json".to_string(),
                    type_: vec!["VerifiableCredential".to_string()],
                    credential_metadata: CredentialMetadata {
                        display: Some(vec![CredentialConfigurationsSupportedDisplay {
                            name: "Verifiable Credential".to_string(),
                            locale: Some("en".to_string()),
                            logo: Some(Logo {
                                uri: "https://www.impierce.com/external/impierce-logo.png".parse().unwrap(),
                                alt_text: Some("Impierce Logo".to_string()),
                            }),
                            description: None,
                            background_image: None,
                            background_color: None,
                            text_color: None,
                        }]),
                        claims: None,
                    },
                    authorization: Authorization {
                        pre_authorized: true,
                        tx_code_constraints: None,
                    },
                },
                provisioned: false,
            })
            .then_expect_events(vec![ServerConfigEvent::CredentialConfigurationUpdated {
                credential_configuration_id,
                credential_issuer_metadata: credential_issuer_metadata_with_credential_configuration,
                credential_configurations,
            }]);
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use crate::credential::aggregate::test_utils::JWT_VC_JSON_VC1_1_CREDENTIAL_CONFIGURATION;
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
                JWT_VC_JSON_VC1_1_CREDENTIAL_CONFIGURATION.clone(),
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
