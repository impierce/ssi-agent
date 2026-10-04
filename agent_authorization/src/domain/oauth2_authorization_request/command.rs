use oid4vci::{authorization_request::AuthorizationRequest, InteractionType};
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

// TODO: remove this clippy allow
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OAuth2AuthorizationRequestCommand {
    CreateOAuth2AuthorizationRequest {
        oauth2_authorization_request_id: String,
        pushed_authorization_request: AuthorizationRequest,
        expires_at: i64,
        #[serde(default)]
        interaction_type: Option<InteractionType>,
    },
    GrantConsent,
    RejectConsent,
    SubmitOpenId4VpResponse {
        openid4vp_response: serde_json::Value,
    },
}

impl CommandOperation for OAuth2AuthorizationRequestCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateOAuth2AuthorizationRequest { .. } => "authorization.oauth2_authorization_requests.create",
            Self::GrantConsent => "authorization.oauth2_authorization_requests.consent.grant",
            Self::RejectConsent => "authorization.oauth2_authorization_requests.consent.reject",
            Self::SubmitOpenId4VpResponse { .. } => {
                "authorization.oauth2_authorization_requests.openid4vp_response.submit"
            }
        }
    }
}
