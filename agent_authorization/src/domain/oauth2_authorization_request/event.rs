use cqrs_es::DomainEvent;
use oid4vci::authorization_details::AuthorizationDetailsObject;
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

use crate::domain::oauth2_authorization_request::aggregate::ConsentStatus;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum OAuth2AuthorizationRequestEvent {
    AuthorizationRequestPushed {
        oauth2_authorization_request_id: String,
        response_type: String,
        state: String,
        client_id: String,
        redirect_uri: Option<Url>,
        scope: String,
        issuer_state: Option<String>,

        // OID4VCI
        authorization_details: Vec<AuthorizationDetailsObject>,

        // PKCE
        #[serde(default)]
        code_challenge: Option<String>,
        #[serde(default)]
        code_challenge_method: Option<String>,

        expires_at: i64,
    },
    ConsentGranted {
        oauth2_authorization_request_id: String,
        consent_status: ConsentStatus,
    },
    ConsentRejected {
        oauth2_authorization_request_id: String,
        consent_status: ConsentStatus,
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
