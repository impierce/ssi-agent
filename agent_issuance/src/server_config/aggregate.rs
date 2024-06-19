use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::server_config::command::ServerConfigCommand;
use crate::server_config::error::ServerConfigError;
use crate::server_config::event::ServerConfigEvent;
use crate::server_config::services::ServerConfigServices;

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
    type Services = ServerConfigServices;

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

            CreateCredentialConfiguration {
                credential_configurations_supported,
            } => Ok(vec![CredentialConfigurationCreated {
                credential_configurations_supported,
            }]),
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
            CredentialConfigurationCreated {
                credential_configurations_supported,
            } => {
                self.credential_issuer_metadata.credential_configurations_supported =
                    credential_configurations_supported
            }
        }
    }
}

#[cfg(test)]
pub mod server_config_tests {
    use std::collections::HashMap;

    use super::*;

    use lazy_static::lazy_static;
    use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
    use serde_json::json;

    use cqrs_es::test::TestFramework;

    use crate::server_config::aggregate::ServerConfig;
    use crate::server_config::event::ServerConfigEvent;

    type ServerConfigTestFramework = TestFramework<ServerConfig>;

    #[test]
    fn test_load_server_metadata() {
        ServerConfigTestFramework::with(ServerConfigServices)
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
        ServerConfigTestFramework::with(ServerConfigServices)
            .given(vec![ServerConfigEvent::ServerMetadataInitialized {
                authorization_server_metadata: AUTHORIZATION_SERVER_METADATA.clone(),
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            }])
            .when(ServerConfigCommand::CreateCredentialConfiguration {
                credential_configurations_supported: CREDENTIAL_CONFIGURATIONS_SUPPORTED.clone(),
            })
            .then_expect_events(vec![ServerConfigEvent::CredentialConfigurationCreated {
                credential_configurations_supported: CREDENTIAL_CONFIGURATIONS_SUPPORTED.clone(),
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
                        "did:key",
                    ],
                    "credential_signing_alg_values_supported": [
                        "EdDSA"
                    ],
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
