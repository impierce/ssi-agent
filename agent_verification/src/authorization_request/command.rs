use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AuthorizationRequestCommand {
    CreateAuthorizationRequest {
        state: String,
        nonce: String,
        presentation_definition_id: Option<String>,
    },
    SignAuthorizationRequestObject,
}
