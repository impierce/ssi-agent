use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AuthorizationRequestCommand {
    CreateAuthorizationRequest { state: String, nonce: String },
    SignAuthorizationRequestObject,
}
