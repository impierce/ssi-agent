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

#[cfg(test)]
mod event_tests {
    use super::*;

    fn all_variants() -> Vec<ClientEvent> {
        vec![ClientEvent::ClientRegistered {
            client_id: "client_id".to_string(),
            client_secret: Some("client_secret".to_string()),
            client_name: Some("Test Client Application".to_string()),
            logo_uri: Some("https://client.example.test/logo.png".to_string()),
            policy_uri: Some("https://client.example.test/policy".to_string()),
            tos_uri: Some("https://client.example.test/tos".to_string()),
            redirect_uris: vec!["https://client.example.test/cb".parse().unwrap()],
            grant_types: vec!["authorization_code".to_string()],
            response_types: vec!["code".to_string()],
            token_endpoint_auth_method: "none".to_string(),
            require_pkce: true,
            code_challenge_methods_supported: vec![CodeChallengeMethod::S256],
            require_pushed_authorization_request: true,
        }]
    }

    #[test]
    fn round_trips_every_variant() {
        for event in all_variants() {
            let value = serde_json::to_value(&event).unwrap();
            let deserialized: ClientEvent = serde_json::from_value(value).unwrap();
            assert_eq!(event, deserialized);
        }
    }

    #[test]
    fn golden_client_registered() {
        let golden = serde_json::json!({
            "ClientRegistered": {
                "client_id": "client_id",
                "client_secret": "client_secret",
                "client_name": "Test Client Application",
                "logo_uri": "https://client.example.test/logo.png",
                "policy_uri": "https://client.example.test/policy",
                "tos_uri": "https://client.example.test/tos",
                "redirect_uris": ["https://client.example.test/cb"],
                "grant_types": ["authorization_code"],
                "response_types": ["code"],
                "token_endpoint_auth_method": "none",
                "require_pkce": true,
                "code_challenge_methods_supported": ["S256"],
                "require_pushed_authorization_request": true
            }
        });

        let event: ClientEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }
}
