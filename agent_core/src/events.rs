use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Debug)]
pub enum CredentialEvent {
    CredentialCreated { user_id: String, payload: Value },
    CredentialSigned { user_id: String, key_id: String },
}
