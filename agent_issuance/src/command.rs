use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Metadata {
    pub metadata: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub enum IssuanceCommand {
    LoadCredentialTemplate {
        credential_template: serde_json::Value,
    },
    CreateCredentialData {
        credential: serde_json::Value,
    },
    SignCredential,
}
