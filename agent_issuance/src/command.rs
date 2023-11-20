use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
    credentials_supported::CredentialsSupportedObject,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Metadata {
    pub metadata: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub enum IssuanceCommand {
    LoadAuthorizationServerMetadata {
        authorization_server_metadata: AuthorizationServerMetadata,
    },
    LoadCredentialIssuerMetadata {
        credential_issuer_metadata: CredentialIssuerMetadata,
    },
    CreateCredentialsSupported {
        credentials_supported: Vec<CredentialsSupportedObject>,
    },
    CreateCredentialOffer,
    LoadCredentialTemplate {
        credential_template: serde_json::Value,
    },
    CreateCredentialData {
        credential: serde_json::Value,
    },
    SignCredential,
}
