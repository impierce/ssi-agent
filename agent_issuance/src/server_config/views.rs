use crate::server_config::{aggregate::ServerConfig, event::ServerConfigEvent};
use cqrs_es::{EventEnvelope, View};

pub type ServerConfigView = ServerConfig;

impl View<ServerConfig> for ServerConfigView {
    fn update(&mut self, event: &EventEnvelope<ServerConfig>) {
        use ServerConfigEvent::*;

        match &event.payload {
            ServerMetadataInitialized {
                authorization_server_metadata,
                credential_issuer_metadata,
                cryptographic_binding_methods_supported,
                signing_algorithms_supported,
            } => {
                self.authorization_server_metadata = *authorization_server_metadata.clone();
                self.credential_issuer_metadata.clone_from(credential_issuer_metadata);
                self.cryptographic_binding_methods_supported
                    .clone_from(cryptographic_binding_methods_supported);
                self.signing_algorithms_supported
                    .clone_from(signing_algorithms_supported);
            }
            IssuerUrlUpdated {
                authorization_server_metadata,
                credential_issuer_metadata,
            } => {
                self.authorization_server_metadata
                    .clone_from(authorization_server_metadata);
                self.credential_issuer_metadata.clone_from(credential_issuer_metadata);
            }
            IssuerDisplayUpdated {
                credential_issuer_metadata,
            } => {
                self.credential_issuer_metadata.clone_from(credential_issuer_metadata);
            }
            CryptographicBindingMethodsUpdated {
                cryptographic_binding_methods_supported,
                credential_issuer_metadata,
                credential_configurations,
            } => {
                self.cryptographic_binding_methods_supported
                    .clone_from(cryptographic_binding_methods_supported);
                self.credential_issuer_metadata.clone_from(credential_issuer_metadata);
                self.credential_configurations.clone_from(credential_configurations);
            }
            SigningAlgorithmsUpdated {
                signing_algorithms_supported,
                credential_issuer_metadata,
                credential_configurations,
            } => {
                self.signing_algorithms_supported
                    .clone_from(signing_algorithms_supported);
                self.credential_issuer_metadata.clone_from(credential_issuer_metadata);
                self.credential_configurations.clone_from(credential_configurations);
            }
            CredentialConfigurationUpdated {
                credential_configuration_id: _,
                credential_issuer_metadata,
                credential_configurations,
            } => {
                self.credential_issuer_metadata.clone_from(credential_issuer_metadata);
                self.credential_configurations.clone_from(credential_configurations);
            }
            CredentialConfigurationRemoved {
                credential_configuration_id: _,
                credential_issuer_metadata,
                credential_configurations,
            } => {
                self.credential_issuer_metadata.clone_from(credential_issuer_metadata);
                self.credential_configurations.clone_from(credential_configurations);
            }
        }
    }
}
