use serde::Deserialize;

use super::entity::Data;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CredentialCommand {
    CreateUnsignedCredential {
        data: Data,
        credential_format_template: serde_json::Value,
    },
    CreateSignedCredential {
        signed_credential: serde_json::Value,
    },
    SignCredential {
        subject_id: String,
        // When true, a credential will be re-signed if it already exists.
        overwrite: bool,
    },
}
