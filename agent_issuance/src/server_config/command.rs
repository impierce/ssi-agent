use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
    credentials_supported::CredentialsSupportedObject,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ServerConfigCommand {
    InitializeServerMetadata {
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: CredentialIssuerMetadata,
    },
    CreateCredentialsSupported {
        credentials_supported: Vec<CredentialsSupportedObject>,
    },
}
