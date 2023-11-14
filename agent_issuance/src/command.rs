use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Metadata {
    pub credential_type: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub enum IssuanceCommand {
    CreateCredentialData {
        // Credential data describing the subject.
        credential_subject: serde_json::Value,
        metadata: Metadata,
    },
    SignCredential,
}
