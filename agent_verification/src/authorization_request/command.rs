use oid4vp::dcql::dcql_query::DcqlQuery;
use serde::Deserialize;

use crate::generic_oid4vc::GenericAuthorizationResponse;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AuthorizationRequestCommand {
    CreateAuthorizationRequest {
        state: String,
        nonce: String,
        dcql_query: Option<DcqlQuery>,
        // If set to `None`, the default response mode will be used (which is currently `direct_post`).
        alternative_response_mode: Option<String>,
    },
    SignAuthorizationRequestObject,
    VerifyAuthorizationResponse {
        authorization_response: GenericAuthorizationResponse,
    },
}
