use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Metadata {
    pub credential_type: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub enum IssuanceCommand {
    Credential,
    CreateCredentialData {
        // Credential data describing the subject.
        credential_subject: serde_json::Value,
        metadata: Metadata,
    },
    SignCredential,
}
