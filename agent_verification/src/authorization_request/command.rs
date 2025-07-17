use oid4vp::dcql::dcql_query::DcqlQuery;
use serde::Deserialize;

use crate::generic_oid4vc::{GenericAuthorizationRequest, GenericAuthorizationResponse};

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AuthorizationRequestCommand {
    CreateAuthorizationRequest {
        state: String,
        nonce: String,
        dcql_query: Option<DcqlQuery>,
        // presentation_definition: Option<PresentationDefinition>,
    },
    SignAuthorizationRequestObject,
    VerifyAuthorizationResponse {
        authorization_request: GenericAuthorizationRequest,
        authorization_response: GenericAuthorizationResponse,
    },
}
