use agent_shared::config::{config, Authorization};
use agent_shared::UrlAppendHelpers as _;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cqrs_es::{event_sink::EventSink, Aggregate};
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

impl Aggregate for ServerConfig {
    type Command = ServerConfigCommand;
    type Event = ServerConfigEvent;
    type Error = ServerConfigError;
    type Services = Arc<IssuanceServices>;

    const TYPE: &'static str = "server_config";

    async fn handle(
        &mut self,
        command: Self::Command,
        _services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use ServerConfigCommand::*;
        use ServerConfigError::*;
        use ServerConfigEvent::*;

        info!("Handling command: {:?}", command);

        let events: Vec<Self::Event> = match command {
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
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
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

    async fn handle(
        given: Vec<ServerConfigEvent>,
        command: ServerConfigCommand,
    ) -> Result<Vec<ServerConfigEvent>, ServerConfigError> {
        ServerConfigTestFramework::with(IssuanceServices::default().await)
            .given(given)
            .when(command)
            .inspect_result()
    }

    /// Applies the events to both the aggregate and its view, and asserts that the view stays an exact projection.
    fn apply_all(events: Vec<ServerConfigEvent>) -> ServerConfig {
        use crate::server_config::views::ServerConfigView;
        use cqrs_es::{EventEnvelope, View as _};

        let mut server_config = ServerConfig::default();
        let mut view = ServerConfigView::default();
        for (sequence, event) in events.into_iter().enumerate() {
            view.update(&EventEnvelope {
                aggregate_id: "server-config".to_string(),
                sequence: sequence + 1,
                payload: event.clone(),
                metadata: HashMap::new(),
            });
            server_config.apply(event);
        }

        assert_eq!(
            serde_json::to_value(&view).unwrap(),
            serde_json::to_value(&server_config).unwrap()
        );
        server_config
    }

    fn initialized(
        authorization_server_metadata: AuthorizationServerMetadata,
        credential_issuer_metadata: CredentialIssuerMetadata,
    ) -> ServerConfigEvent {
        ServerConfigEvent::ServerMetadataInitialized {
            authorization_server_metadata: Box::new(authorization_server_metadata),
            credential_issuer_metadata: Box::new(credential_issuer_metadata),
            cryptographic_binding_methods_supported: cryptographic_binding_methods_supported(),
            signing_algorithms_supported: signing_algorithms_supported(),
        }
    }

    fn credential_configuration(format: &str) -> CredentialConfiguration {
        CredentialConfiguration {
            credential_configuration_id: credential_configuration_id(),
            format: format.to_string(),
            type_: vec!["VerifiableCredential".to_string()],
            credential_metadata: CredentialMetadata {
                display: None,
                claims: None,
            },
            authorization: Authorization::default(),
        }
    }

    /// Returns the events of a server config holding a single `jwt_vc_json` credential configuration.
    async fn with_credential_configuration(provisioned: bool) -> Vec<ServerConfigEvent> {
        let initialized = initialized(
            *authorization_server_metadata(static_issuer_url()),
            *credential_issuer_metadata(static_issuer_url()),
        );
        let updated = handle(
            vec![initialized.clone()],
            ServerConfigCommand::UpdateCredentialConfiguration {
                credential_configuration: credential_configuration("jwt_vc_json"),
                provisioned,
            },
        )
        .await
        .unwrap();

        [vec![initialized], updated].concat()
    }

    #[rstest]
    async fn updating_the_issuer_url_moves_every_endpoint() {
        let old_url = static_issuer_url();
        let new_url: url::Url = "https://new-domain.example.org/unicore/".parse().unwrap();
        let initialized = initialized(
            AuthorizationServerMetadata {
                issuer: old_url.clone(),
                authorization_endpoint: Some(old_url.join("auth/authorize").unwrap()),
                token_endpoint: Some(old_url.join("auth/token").unwrap()),
                pushed_authorization_request_endpoint: Some(old_url.join("auth/par").unwrap()),
                interactive_authorization_endpoint: Some(old_url.join("auth/par").unwrap()),
                ..Default::default()
            },
            CredentialIssuerMetadata {
                credential_issuer: old_url.clone(),
                credential_endpoint: old_url.join("openid4vci/credential").unwrap(),
                nonce_endpoint: Some(old_url.join("openid4vci/nonce").unwrap()),
                ..Default::default()
            },
        );

        let events = handle(
            vec![initialized.clone()],
            ServerConfigCommand::UpdateIssuerUrl { url: new_url.clone() },
        )
        .await
        .unwrap();
        let server_config = apply_all([vec![initialized], events].concat());

        let endpoint = |path: &str| Some(new_url.join(path).unwrap());
        let authorization_server_metadata = server_config.authorization_server_metadata;
        assert_eq!(authorization_server_metadata.issuer, new_url);
        assert_eq!(
            authorization_server_metadata.authorization_endpoint,
            endpoint("auth/authorize")
        );
        assert_eq!(authorization_server_metadata.token_endpoint, endpoint("auth/token"));
        assert_eq!(
            authorization_server_metadata.pushed_authorization_request_endpoint,
            endpoint("auth/par")
        );
        assert_eq!(
            authorization_server_metadata.interactive_authorization_endpoint,
            endpoint("auth/par")
        );

        let credential_issuer_metadata = server_config.credential_issuer_metadata;
        assert_eq!(credential_issuer_metadata.credential_issuer, new_url);
        assert_eq!(
            Some(credential_issuer_metadata.credential_endpoint),
            endpoint("openid4vci/credential")
        );
        assert_eq!(credential_issuer_metadata.nonce_endpoint, endpoint("openid4vci/nonce"));
    }

    #[rstest]
    async fn updating_the_issuer_url_does_not_add_unsupported_endpoints() {
        let initialized = initialized(
            AuthorizationServerMetadata {
                issuer: static_issuer_url(),
                ..Default::default()
            },
            *credential_issuer_metadata(static_issuer_url()),
        );

        let events = handle(
            vec![initialized.clone()],
            ServerConfigCommand::UpdateIssuerUrl {
                url: "https://new-domain.example.org/".parse().unwrap(),
            },
        )
        .await
        .unwrap();
        let server_config = apply_all([vec![initialized], events].concat());

        let authorization_server_metadata = server_config.authorization_server_metadata;
        assert_eq!(authorization_server_metadata.authorization_endpoint, None);
        assert_eq!(authorization_server_metadata.token_endpoint, None);
        assert_eq!(
            authorization_server_metadata.pushed_authorization_request_endpoint,
            None
        );
        assert_eq!(authorization_server_metadata.interactive_authorization_endpoint, None);
        assert_eq!(server_config.credential_issuer_metadata.nonce_endpoint, None);
    }

    #[rstest]
    async fn updating_the_issuer_display() {
        let initialized = initialized(
            *authorization_server_metadata(static_issuer_url()),
            *credential_issuer_metadata(static_issuer_url()),
        );
        let display = Some(vec![serde_json::json!({ "name": "UniCore", "locale": "en" })]);

        let events = handle(
            vec![initialized.clone()],
            ServerConfigCommand::UpdateIssuerDisplay {
                display: display.clone(),
            },
        )
        .await
        .unwrap();

        assert_eq!(
            apply_all([vec![initialized], events].concat())
                .credential_issuer_metadata
                .display,
            display
        );
    }

    #[rstest]
    async fn binding_methods_and_signing_algorithms_apply_to_every_credential_configuration() {
        let given = with_credential_configuration(false).await;

        let binding_methods_updated = handle(
            given.clone(),
            ServerConfigCommand::UpdateCryptographicBindingMethods {
                cryptographic_binding_methods_supported: vec!["did:web".to_string()],
            },
        )
        .await
        .unwrap();
        let given = [given, binding_methods_updated].concat();

        let signing_algorithms_updated = handle(
            given.clone(),
            ServerConfigCommand::UpdateSigningAlgorithms {
                signing_algorithms_supported: vec![Algorithm::EdDSA],
            },
        )
        .await
        .unwrap();
        let server_config = apply_all([given, signing_algorithms_updated].concat());

        assert_eq!(server_config.cryptographic_binding_methods_supported, vec!["did:web"]);
        assert_eq!(server_config.signing_algorithms_supported, vec![Algorithm::EdDSA]);

        let (_, credential_configuration, _) = &server_config.credential_configurations[&credential_configuration_id()];
        assert_eq!(
            server_config
                .credential_issuer_metadata
                .credential_configurations_supported[&credential_configuration_id()],
            *credential_configuration
        );
        assert_eq!(
            credential_configuration.cryptographic_binding_methods_supported,
            vec!["did:web"]
        );
        assert_eq!(
            credential_configuration.credential_signing_alg_values_supported,
            vec![AlgIdentifier::String("EdDSA".to_string())]
        );
        assert_eq!(
            credential_configuration.proof_types_supported[&ProofType::Jwt].proof_signing_alg_values_supported,
            vec![AlgIdentifier::String("EdDSA".to_string())]
        );
    }

    #[rstest]
    async fn sd_jwt_credential_configurations_are_supported() {
        let initialized = initialized(
            *authorization_server_metadata(static_issuer_url()),
            *credential_issuer_metadata(static_issuer_url()),
        );

        for format in ["vc+sd-jwt", "dc+sd-jwt"] {
            let events = handle(
                vec![initialized.clone()],
                ServerConfigCommand::UpdateCredentialConfiguration {
                    credential_configuration: credential_configuration(format),
                    provisioned: false,
                },
            )
            .await
            .unwrap();
            let server_config = apply_all([vec![initialized.clone()], events].concat());

            let (_, credential_configuration, _) =
                &server_config.credential_configurations[&credential_configuration_id()];
            let credential_configuration = serde_json::to_value(credential_configuration).unwrap();
            assert_eq!(credential_configuration["format"], format);

            if format == "dc+sd-jwt" {
                let vct = format!("vct/{}/0", URL_SAFE_NO_PAD.encode(credential_configuration_id()));
                assert!(
                    credential_configuration["vct"].as_str().unwrap().ends_with(&vct),
                    "{credential_configuration}"
                );
            }
        }
    }

    #[rstest]
    async fn unsupported_credential_formats_are_rejected() {
        let result = handle(
            vec![initialized(
                *authorization_server_metadata(static_issuer_url()),
                *credential_issuer_metadata(static_issuer_url()),
            )],
            ServerConfigCommand::UpdateCredentialConfiguration {
                credential_configuration: credential_configuration("ldp_vc"),
                provisioned: false,
            },
        )
        .await;

        assert!(matches!(
            result,
            Err(ServerConfigError::UnsupportedCredentialFormatIdentifierError(_))
        ));
    }

    #[rstest]
    async fn provisioned_credential_configurations_cannot_be_changed_at_runtime() {
        let given = with_credential_configuration(true).await;

        let result = handle(
            given.clone(),
            ServerConfigCommand::UpdateCredentialConfiguration {
                credential_configuration: credential_configuration("jwt_vc_json"),
                provisioned: false,
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(ServerConfigError::UpdateProvisionedCredentialConfigurationError)
        ));

        let result = handle(
            given.clone(),
            ServerConfigCommand::RemoveCredentialConfiguration {
                credential_configuration_id: credential_configuration_id(),
                provisioned: false,
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(ServerConfigError::RemoveProvisionedCredentialConfigurationError)
        ));

        let events = handle(
            given.clone(),
            ServerConfigCommand::RemoveCredentialConfiguration {
                credential_configuration_id: credential_configuration_id(),
                provisioned: true,
            },
        )
        .await
        .unwrap();
        let server_config = apply_all([given, events].concat());
        assert!(server_config.credential_configurations.is_empty());
        assert!(server_config
            .credential_issuer_metadata
            .credential_configurations_supported
            .is_empty());
    }

    #[rstest]
    async fn runtime_credential_configurations_can_be_replaced_and_removed() {
        let given = with_credential_configuration(false).await;

        let mut replacement = credential_configuration("jwt_vc_json");
        replacement.type_.push("OpenBadgeCredential".to_string());
        let events = handle(
            given.clone(),
            ServerConfigCommand::UpdateCredentialConfiguration {
                credential_configuration: replacement,
                provisioned: false,
            },
        )
        .await
        .unwrap();
        let given = [given, events].concat();
        let server_config = apply_all(given.clone());
        assert_eq!(server_config.credential_configurations.len(), 1);
        assert!(serde_json::to_string(&server_config.credential_configurations)
            .unwrap()
            .contains("OpenBadgeCredential"));

        let events = handle(
            given.clone(),
            ServerConfigCommand::RemoveCredentialConfiguration {
                credential_configuration_id: credential_configuration_id(),
                provisioned: false,
            },
        )
        .await
        .unwrap();
        assert!(apply_all([given, events].concat()).credential_configurations.is_empty());
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
