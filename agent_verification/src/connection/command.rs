use crate::generic_oid4vc::{GenericAuthorizationRequest, GenericAuthorizationResponse};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ConnectionCommand {
    VerifyAuthorizationResponse {
        authorization_request: GenericAuthorizationRequest,
        authorization_response: GenericAuthorizationResponse,
    },
}
