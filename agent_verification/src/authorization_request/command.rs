use oid4vp::dcql::dcql_query::DcqlQuery;
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

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

impl CommandOperation for AuthorizationRequestCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateAuthorizationRequest { .. } => "verification.authorization_requests.create",
            Self::SignAuthorizationRequestObject => "verification.authorization_requests.sign",
            Self::VerifyAuthorizationResponse { .. } => "verification.authorization_requests.response.verify",
        }
    }
}
