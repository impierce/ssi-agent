use oid4vc_core::authorization_response::AuthorizationResponse;
use serde::Deserialize;
use siopv2::siopv2::SIOPv2;

use super::aggregate::SIOPv2AuthorizationRequest;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ConnectionCommand {
    VerifySIOPv2AuthorizationResponse {
        siopv2_authorization_request: SIOPv2AuthorizationRequest,
        siopv2_authorization_response: AuthorizationResponse<SIOPv2>,
        connection_notification_uri: Option<url::Url>,
    },
}
