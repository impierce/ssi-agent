use cqrs_es::DomainEvent;
use oid4vci::authorization_request::CodeChallengeMethod;
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum AuthorizationCodeEvent {
    AuthorizationCodeCreated {
        authorization_code_id: String,
        client_id: String,
        redirect_uri: Option<Url>,
        code_challenge: Option<String>,
        code_challenge_method: Option<CodeChallengeMethod>,
        issuer_state: Option<String>,
        expires_at: i64,
    },
    AuthorizationCodeRedeemed {
        authorization_code_id: String,
        redeemed: bool,
    },
}

impl DomainEvent for AuthorizationCodeEvent {
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

    fn all_variants() -> Vec<AuthorizationCodeEvent> {
        vec![
            AuthorizationCodeEvent::AuthorizationCodeCreated {
                authorization_code_id: "authorization_code_id".to_string(),
                client_id: "client_id".to_string(),
                redirect_uri: Some("https://client.example.test/cb".parse().unwrap()),
                code_challenge: Some("code_challenge".to_string()),
                code_challenge_method: Some(CodeChallengeMethod::S256),
                issuer_state: Some("issuer_state".to_string()),
                expires_at: 600,
            },
            AuthorizationCodeEvent::AuthorizationCodeRedeemed {
                authorization_code_id: "authorization_code_id".to_string(),
                redeemed: true,
            },
        ]
    }

    #[test]
    fn round_trips_every_variant() {
        for event in all_variants() {
            let value = serde_json::to_value(&event).unwrap();
            let deserialized: AuthorizationCodeEvent = serde_json::from_value(value).unwrap();
            assert_eq!(event, deserialized);
        }
    }

    #[test]
    fn golden_authorization_code_created() {
        let golden = serde_json::json!({
            "AuthorizationCodeCreated": {
                "authorization_code_id": "authorization_code_id",
                "client_id": "client_id",
                "redirect_uri": "https://client.example.test/cb",
                "code_challenge": "code_challenge",
                "code_challenge_method": "S256",
                "issuer_state": "issuer_state",
                "expires_at": 600
            }
        });

        let event: AuthorizationCodeEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_authorization_code_redeemed() {
        let golden = serde_json::json!({
            "AuthorizationCodeRedeemed": {
                "authorization_code_id": "authorization_code_id",
                "redeemed": true
            }
        });

        let event: AuthorizationCodeEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }
}
