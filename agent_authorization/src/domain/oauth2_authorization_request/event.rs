use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

use crate::application::pushed_authorization_service::PushedAuthorizationRequest;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum OAuth2AuthorizationRequestEvent {
    AuthorizationRequestPushed {
        oauth2_authorization_request_id: String,
        response_type: String,
        state: String,
        client_id: String,
        redirect_uri: String,
        scope: String,
        #[serde(default)]
        client_assertion_type: Option<String>,
        #[serde(default)]
        client_assertion: Option<String>,
        issuer_state: Option<String>,

        // OID4VCI
        // authorization_details: AuthorizationDetailsObject,

        // PKCE
        #[serde(default)]
        code_challenge: Option<String>,
        #[serde(default)]
        code_challenge_method: Option<String>,

        expires_at: i64,
    },
}

impl DomainEvent for OAuth2AuthorizationRequestEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
