use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CredentialCommand {
    LoadCredentialFormatTemplate {
        credential_format_template: serde_json::Value,
    },
    CreateUnsignedCredential {
        // subject: Subject,
        credential: serde_json::Value,
    },
}
