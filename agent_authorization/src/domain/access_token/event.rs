use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum AccessTokenEvent {
    AccessTokenIssued {
        access_token_id: String,
        user_id: String,
        client_id: String,
        scopes: Option<String>,
        issued_at: u64,
        access_token_expires_at: u64,
        refresh_token_expires_at: Option<u64>,
        issuer_state: Option<String>,
    },
}

impl DomainEvent for AccessTokenEvent {
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

    fn all_variants() -> Vec<AccessTokenEvent> {
        vec![AccessTokenEvent::AccessTokenIssued {
            access_token_id: "access_token_id".to_string(),
            user_id: "user_id".to_string(),
            client_id: "client_id".to_string(),
            scopes: Some("openid profile email".to_string()),
            issued_at: 0,
            access_token_expires_at: 3600,
            refresh_token_expires_at: Some(86400),
            issuer_state: Some("issuer_state".to_string()),
        }]
    }

    #[test]
    fn round_trips_every_variant() {
        for event in all_variants() {
            let value = serde_json::to_value(&event).unwrap();
            let deserialized: AccessTokenEvent = serde_json::from_value(value).unwrap();
            assert_eq!(event, deserialized);
        }
    }

    #[test]
    fn golden_access_token_issued() {
        let golden = serde_json::json!({
            "AccessTokenIssued": {
                "access_token_id": "access_token_id",
                "user_id": "user_id",
                "client_id": "client_id",
                "scopes": "openid profile email",
                "issued_at": 0,
                "access_token_expires_at": 3600,
                "refresh_token_expires_at": 86400,
                "issuer_state": "issuer_state"
            }
        });

        let event: AccessTokenEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }
}
