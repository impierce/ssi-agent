use cqrs_es::DomainEvent;
use oid4vci::authorization_request::CodeChallengeMethod;
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum ClientEvent {
    ClientRegistered {
        client_id: String,
        client_secret: Option<String>,
        client_name: Option<String>,
        logo_uri: Option<String>,
        policy_uri: Option<String>,
        tos_uri: Option<String>,
        redirect_uris: Vec<Url>,
        grant_types: Vec<String>,
        response_types: Vec<String>,
        token_endpoint_auth_method: String,
        require_pkce: bool,
        code_challenge_methods_supported: Vec<CodeChallengeMethod>,
        require_pushed_authorization_request: bool,
    },
}

impl DomainEvent for ClientEvent {
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
