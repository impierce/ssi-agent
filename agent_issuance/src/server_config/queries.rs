use cqrs_es::{EventEnvelope, View};
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
};
use serde::{Deserialize, Serialize};

use crate::server_config::{aggregate::ServerConfig, event::ServerConfigEvent};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ServerConfigView {
    pub authorization_server_metadata: AuthorizationServerMetadata,
    pub credential_issuer_metadata: Option<CredentialIssuerMetadata>,
}

impl View<ServerConfig> for ServerConfigView {
    fn update(&mut self, event: &EventEnvelope<ServerConfig>) {
        use ServerConfigEvent::*;

        match &event.payload {
            ServerMetadataInitialized {
                authorization_server_metadata,
                credential_issuer_metadata,
            } => {
                self.authorization_server_metadata = *authorization_server_metadata.clone();
                self.credential_issuer_metadata
                    .replace(credential_issuer_metadata.clone());
            }
            CredentialsSupportedCreated {
                credential_configurations_supported,
            } => {
                if let Some(credential_issuer_metadata) = self.credential_issuer_metadata.as_mut() {
                    credential_issuer_metadata.credential_configurations_supported =
                        credential_configurations_supported.clone();
                }
            }
        }
    }
}
