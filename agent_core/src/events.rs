use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum CredentialEvent {
    CredentialCreated {
        user_id: Option<String>,
        timestamp: String,
        payload: Value,
    },
    CredentialSigned {
        user_id: Option<String>,
        timestamp: String,
        payload: Value,
    },
}
