use serde::Deserialize;

use super::entity::Data;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CredentialCommand {
    CreateUnsignedCredential {
        data: Data,
        credential_format_template: serde_json::Value,
    },
}
