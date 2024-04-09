use std::collections::HashMap;

use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata,
    credential_configurations_supported::CredentialConfigurationsSupportedObject,
    credential_issuer_metadata::CredentialIssuerMetadata,
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
        credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject>,
    },
}
