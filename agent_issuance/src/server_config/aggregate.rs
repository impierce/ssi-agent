use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
};
use serde::{Deserialize, Serialize};

use crate::server_config::command::ServerConfigCommand;
use crate::server_config::error::ServerConfigError;
use crate::server_config::event::ServerConfigEvent;
use crate::server_config::services::ServerConfigServices;

/// An aggregate that holds the configuration of the server.
#[derive(Clone, Default, Deserialize, Serialize, Debug)]
pub struct ServerConfig {
    // TODO: These fields should not be optional. ServerConfig should be created with all of its fields that can be
    // updated through commands.
    authorization_server_metadata: Option<AuthorizationServerMetadata>,
    credential_issuer_metadata: Option<CredentialIssuerMetadata>,
}

#[derive(Clone, Default, Deserialize, Serialize, Debug)]
pub struct ServerConfigAlt {
    // root: Root,
    id: uuid::Uuid,
    // Value Objects
    authorization_server_metadata: Option<AuthorizationServerMetadata>,
    credential_issuer_metadata: Option<CredentialIssuerMetadata>,
    // Entities
    // issuer_logo: Image,
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
        use ServerConfigError::*;
        use ServerConfigEvent::*;

        match command {
            LoadAuthorizationServerMetadata {
                authorization_server_metadata,
            } => Ok(vec![AuthorizationServerMetadataLoaded {
                authorization_server_metadata,
            }]),
            LoadCredentialIssuerMetadata {
                credential_issuer_metadata,
            } => Ok(vec![CredentialIssuerMetadataLoaded {
                credential_issuer_metadata,
            }]),
            CreateCredentialsSupported { credentials_supported } => {
                self.credential_issuer_metadata
                    .as_ref()
                    .ok_or(MissingCredentialIssuerMetadataError)?;
                Ok(vec![CredentialsSupportedCreated { credentials_supported }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use ServerConfigEvent::*;

        match event {
            AuthorizationServerMetadataLoaded {
                authorization_server_metadata,
            } => {
                self.authorization_server_metadata
                    .replace(*authorization_server_metadata);
            }
            CredentialIssuerMetadataLoaded {
                credential_issuer_metadata,
            } => {
                self.credential_issuer_metadata.replace(credential_issuer_metadata);
            }
            CredentialsSupportedCreated { credentials_supported } => {
                self.credential_issuer_metadata.as_mut().unwrap().credentials_supported = credentials_supported
            }
        }
    }
}

#[cfg(test)]
pub mod server_config_tests {
    use super::*;

    use lazy_static::lazy_static;
    use oid4vci::credential_issuer::credentials_supported::CredentialsSupportedObject;
    use serde_json::json;

    use cqrs_es::test::TestFramework;

    use crate::server_config::aggregate::ServerConfig;
    use crate::server_config::event::ServerConfigEvent;

    type ServerConfigTestFramework = TestFramework<ServerConfig>;

    #[test]
    fn test_load_authorization_server_metadata() {
        ServerConfigTestFramework::with(ServerConfigServices)
            .given_no_previous_events()
            .when(ServerConfigCommand::LoadAuthorizationServerMetadata {
                authorization_server_metadata: Box::new(AUTHORIZATION_SERVER_METADATA.clone()),
            })
            .then_expect_events(vec![ServerConfigEvent::AuthorizationServerMetadataLoaded {
                authorization_server_metadata: Box::new(AUTHORIZATION_SERVER_METADATA.clone()),
            }]);
    }

    #[test]
    fn test_load_credential_issuer_metadata() {
        ServerConfigTestFramework::with(ServerConfigServices)
            .given_no_previous_events()
            .when(ServerConfigCommand::LoadCredentialIssuerMetadata {
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            })
            .then_expect_events(vec![ServerConfigEvent::CredentialIssuerMetadataLoaded {
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            }]);
    }

    #[test]
    fn test_create_credentials_supported() {
        ServerConfigTestFramework::with(ServerConfigServices)
            .given(vec![ServerConfigEvent::CredentialIssuerMetadataLoaded {
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            }])
            .when(ServerConfigCommand::CreateCredentialsSupported {
                credentials_supported: CREDENTIALS_SUPPORTED.clone(),
            })
            .then_expect_events(vec![ServerConfigEvent::CredentialsSupportedCreated {
                credentials_supported: CREDENTIALS_SUPPORTED.clone(),
            }]);
    }

    lazy_static! {
        static ref BASE_URL: url::Url = "https://example.com/".parse().unwrap();
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
        pub static ref AUTHORIZATION_SERVER_METADATA: AuthorizationServerMetadata = AuthorizationServerMetadata {
            issuer: BASE_URL.clone(),
            token_endpoint: Some(BASE_URL.join("token").unwrap()),
            ..Default::default()
        };
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
}
