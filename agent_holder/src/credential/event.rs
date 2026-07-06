use cqrs_es::DomainEvent;
use identity_credential::credential::Jwt;
use serde::{Deserialize, Serialize};
use strum::Display;

use super::aggregate::Data;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum CredentialEvent {
    CredentialAdded {
        holder_credential_id: String,
        received_offer_id: Option<String>,
        credential: Jwt,
        data: Data,
    },
}

impl DomainEvent for CredentialEvent {
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

/// Wire-format regression tests: every variant is round-tripped through JSON and checked
/// against a checked-in "golden" JSON literal. If a golden fixture stops matching, either the
/// change is breaking (bump `event_version` and add an upcaster, see `docs/event-versioning.md`)
/// or the fixture needs deliberate updating.
#[cfg(test)]
mod wire_format_tests {
    use super::*;
    use serde_json::json;

    /// Asserts that `event` serializes to exactly `golden`, that it round-trips losslessly
    /// through JSON, and that the golden fixture itself still deserializes into `event`.
    fn assert_round_trip_and_golden(event: CredentialEvent, golden: serde_json::Value) {
        let serialized = serde_json::to_value(&event).expect("event should serialize");
        assert_eq!(serialized, golden, "serialized event drifted from the golden fixture");

        let round_tripped: CredentialEvent =
            serde_json::from_value(serialized).expect("serialized event should deserialize");
        assert_eq!(round_tripped, event, "round-trip through JSON changed the event");

        let from_golden: CredentialEvent =
            serde_json::from_value(golden).expect("golden fixture should deserialize");
        assert_eq!(from_golden, event, "golden fixture no longer deserializes into the expected event");
    }

    #[test]
    fn credential_added() {
        let event = CredentialEvent::CredentialAdded {
            holder_credential_id: "holder-credential-id".to_string(),
            received_offer_id: Some("received-offer-id".to_string()),
            credential: Jwt::from("eyJhbGciOiJFZERTQSJ9.eyJzdWIiOiJ0ZXN0In0.sig".to_string()),
            data: Data {
                raw: json!({"first_name": "Ferris"}),
            },
        };
        let golden = json!({
            "CredentialAdded": {
                "holder_credential_id": "holder-credential-id",
                "received_offer_id": "received-offer-id",
                "credential": "eyJhbGciOiJFZERTQSJ9.eyJzdWIiOiJ0ZXN0In0.sig",
                "data": { "raw": {"first_name": "Ferris"} }
            }
        });
        assert_round_trip_and_golden(event, golden);
    }
}
