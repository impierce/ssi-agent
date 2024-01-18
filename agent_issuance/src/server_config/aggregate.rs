use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
};
use serde::{Deserialize, Serialize};

use crate::server_config::command::ServerConfigCommand;
use crate::server_config::entity::{Image, Root};
use crate::server_config::error::ServerConfigError;
use crate::server_config::event::ServerConfigEvent;
use crate::server_config::services::ServerConfigServices;

/// An aggregate that holds the configuration of the server.
#[derive(Clone, Default, Deserialize, Serialize, Debug)]
pub struct ServerConfig {
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
        use ServerConfigError::*;

        match command {
            ServerConfigCommand::LoadAuthorizationServerMetadata {
                authorization_server_metadata,
            } => Ok(vec![ServerConfigEvent::AuthorizationServerMetadataLoaded {
                authorization_server_metadata,
            }]),
            ServerConfigCommand::LoadCredentialIssuerMetadata {
                credential_issuer_metadata,
            } => Ok(vec![ServerConfigEvent::CredentialIssuerMetadataLoaded {
                credential_issuer_metadata,
            }]),
            ServerConfigCommand::CreateCredentialsSupported { credentials_supported } => {
                self.credential_issuer_metadata
                    .as_ref()
                    .ok_or(MissingCredentialIssuerMetadataError)?;
                Ok(vec![ServerConfigEvent::CredentialsSupportedCreated {
                    credentials_supported,
                }])
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
mod server_config_tests {
    use super::*;

    use async_trait::async_trait;
    use std::sync::Mutex;

    use cqrs_es::test::TestFramework;

    use crate::server_config::aggregate::ServerConfig;
    use crate::server_config::event::ServerConfigEvent;

    type ServerConfigTestFramework = TestFramework<ServerConfig>;

    #[test]
    fn test_load_server_config() {
        let expected = ServerConfigEvent::AuthorizationServerMetadataLoaded {
            authorization_server_metadata: Box::new(AuthorizationServerMetadata {
                issuer: "https://www.example.org".parse().unwrap(),
                token_endpoint: Some(
                    "https://www.example.org"
                        .parse::<url::Url>()
                        .unwrap()
                        .join("token")
                        .unwrap(),
                ),
                ..Default::default()
            }),
        };
        // let command = ServerConfigCommand::LoadAuthorizationServerMetadata { authorization_server_metadata: () }
        // let services = ServerConfigServices::new();
        // ServerConfigTestFramework::with().given_no_previous_events()
        //     .when(command)
        //     .then_expect_events(vec![expected]);
    }
}
