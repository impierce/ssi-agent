use oid4vci::{
    credential_issuer::{
        credential_configurations_supported::CredentialConfigurationsSupportedObject,
        credential_issuer_metadata::CredentialIssuerMetadata,
    },
    token_response::TokenResponse,
};
use serde::Deserialize;

use super::entity::Data;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CredentialCommand {
    AddCredential {
        credential_id: String,
        offer_id: String,
        credential: serde_json::Value,
    },
}
