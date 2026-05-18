use oid4vci::{authorization_request::AuthorizationRequest, InteractionType};
use serde::Deserialize;

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
