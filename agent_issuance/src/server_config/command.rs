use agent_shared::config::CredentialConfiguration;
use jsonwebtoken::Algorithm;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
};
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ServerConfigCommand {
    InitializeServerMetadata {
        // TODO: Move this to `agent_authorization`.
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        cryptographic_binding_methods_supported: Vec<String>,
        signing_algorithms_supported: Vec<Algorithm>,
    },
    UpdateIssuerUrl {
        url: url::Url,
    },
    UpdateIssuerDisplay {
        display: Option<Vec<serde_json::Value>>,
    },
    UpdateCryptographicBindingMethods {
        cryptographic_binding_methods_supported: Vec<String>,
    },
    UpdateSigningAlgorithms {
        signing_algorithms_supported: Vec<Algorithm>,
    },
    UpdateCredentialConfiguration {
        credential_configuration: CredentialConfiguration,
        provisioned: bool,
    },
    RemoveCredentialConfiguration {
        credential_configuration_id: String,
        provisioned: bool,
    },
}

impl CommandOperation for ServerConfigCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::InitializeServerMetadata { .. } => "issuance.configurations.metadata.initialize",
            Self::UpdateIssuerUrl { .. } => "issuance.configurations.issuer_url.update",
            Self::UpdateIssuerDisplay { .. } => "issuance.configurations.issuer_display.update",
            Self::UpdateCryptographicBindingMethods { .. } => {
                "issuance.configurations.cryptographic_binding_methods.update"
            }
            Self::UpdateSigningAlgorithms { .. } => "issuance.configurations.signing_algorithms.update",
            Self::UpdateCredentialConfiguration { .. } => "issuance.configurations.credentials.update",
            Self::RemoveCredentialConfiguration { .. } => "issuance.configurations.credentials.remove",
        }
    }
}
