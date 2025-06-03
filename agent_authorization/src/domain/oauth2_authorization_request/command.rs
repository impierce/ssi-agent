use crate::application::pushed_authorization_service::PushedAuthorizationRequest;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OAuth2AuthorizationRequestCommand {
    InitializeFromPushedAuthorizationRequest {
        oauth2_authorization_request_id: String,
        pushed_authorization_request: PushedAuthorizationRequest,
        expires_at: i64,
    },
}
