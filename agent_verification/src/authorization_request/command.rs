use oid4vp::PresentationDefinition;
use serde::Deserialize;

use crate::generic_oid4vc::{GenericAuthorizationRequest, GenericAuthorizationResponse};

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AuthorizationRequestCommand {
    CreateAuthorizationRequest {
        state: String,
        nonce: String,
        presentation_definition: Option<PresentationDefinition>,
    },
    SignAuthorizationRequestObject,
    VerifyAuthorizationResponse {
        authorization_request: GenericAuthorizationRequest,
        authorization_response: GenericAuthorizationResponse,
    },
}
