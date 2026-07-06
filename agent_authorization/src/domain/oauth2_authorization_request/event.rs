use crate::domain::oauth2_authorization_request::aggregate::ConsentStatus;
use cqrs_es::DomainEvent;
use oid4vci::{authorization_details::AuthorizationDetailsObject, authorization_request::CodeChallengeMethod};
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

// TODO: remove this clippy allow
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum OAuth2AuthorizationRequestEvent {
    OAuth2AuthorizationRequestCreated {
        oauth2_authorization_request_id: String,
        response_type: String,
        state: String,
        client_id: String,
        redirect_uri: Option<Url>,
        scope: Option<String>,
        issuer_state: Option<String>,

        // OID4VCI
        authorization_details: Option<Vec<AuthorizationDetailsObject>>,

        // PKCE
        #[serde(default)]
        code_challenge: Option<String>,
        #[serde(default)]
        code_challenge_method: Option<CodeChallengeMethod>,

        expires_at: i64,

        openid4vp_request: Option<serde_json::Value>,
    },
    OAuth2AuthorizationRequestExpired {
        oauth2_authorization_request_id: String,
        consent_status: ConsentStatus,
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

    // Integer schema version of this event payload. Bump on breaking change and add an upcaster (see docs/event-versioning.md).
    fn event_version(&self) -> String {
        "1".to_string()
    }
}

/// Upcasters migrating old persisted versions of these events to the current
/// schema version. See `docs/event-versioning.md`.
pub fn upcasters() -> Vec<Box<dyn cqrs_es::persist::EventUpcaster>> {
    vec![]
}
