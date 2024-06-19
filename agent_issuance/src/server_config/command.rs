use oid4vci::{
    credential_format_profiles::{CredentialFormats, WithParameters},
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata,
    },
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ServerConfigCommand {
    InitializeServerMetadata {
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: CredentialIssuerMetadata,
    },
    AddCredentialConfiguration {
        credential_configuration_id: String,
        credential_format_with_parameters: CredentialFormats<WithParameters>,
        display: Vec<serde_json::Value>,
    },
}
