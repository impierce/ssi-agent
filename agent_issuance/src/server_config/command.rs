use agent_library::template::event::Authorization;
use jsonwebtoken::Algorithm;
use oid4vci::{
    credential_format_profiles::{CredentialFormats, WithParameters},
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_configurations_supported::{ClaimDescription, CredentialConfigurationsSupportedDisplay},
        credential_issuer_metadata::CredentialIssuerMetadata,
    },
};
use serde::Deserialize;

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

    CreateCredentialConfiguration {
        template_id: String,
        credential_configuration_id: String,
        credential_format_with_parameters: CredentialFormats<WithParameters>,
        display: Vec<CredentialConfigurationsSupportedDisplay>,
        claims: Vec<ClaimDescription>,
        authorization: Authorization,
    },
    UpdateCredentialConfigurationId {
        template_id: String,
        credential_configuration_id: String,
    },
    UpdateCredentialConfigurationDisplay {
        template_id: String,
        display: CredentialConfigurationsSupportedDisplay,
    },
    UpdateCredentialConfigurationAuthorization {
        template_id: String,
        authorization: Authorization,
    },
    RemoveCredentialConfiguration {
        template_id: String,
    },
}
