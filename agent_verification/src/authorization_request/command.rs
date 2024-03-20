use oid4vc_core::client_metadata::ClientMetadata;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AuthorizationRequestCommand {
    CreateAuthorizationRequest {
        client_metadata: Box<ClientMetadata>,
        state: String,
        nonce: String,
    },
    SignAuthorizationRequestObject,
}
