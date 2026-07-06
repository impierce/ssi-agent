use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, Display, PartialEq, Serialize)]
pub enum NonceEvent {
    NonceGenerated { c_nonce: String, is_redeemed: bool },
    NonceRedeemed { c_nonce: String, is_redeemed: bool },
}

impl DomainEvent for NonceEvent {
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

    /// Asserts that `event` serializes to exactly `golden`, that it round-trips losslessly
    /// through JSON, and that the golden fixture itself still deserializes into `event`.
    fn assert_round_trip_and_golden(event: NonceEvent, golden: serde_json::Value) {
        let serialized = serde_json::to_value(&event).expect("event should serialize");
        assert_eq!(serialized, golden, "serialized event drifted from the golden fixture");

        let round_tripped: NonceEvent = serde_json::from_value(serialized).expect("serialized event should deserialize");
        assert_eq!(round_tripped, event, "round-trip through JSON changed the event");

        let from_golden: NonceEvent = serde_json::from_value(golden).expect("golden fixture should deserialize");
        assert_eq!(from_golden, event, "golden fixture no longer deserializes into the expected event");
    }

    #[test]
    fn nonce_generated() {
        let event = NonceEvent::NonceGenerated {
            c_nonce: "test-c-nonce".to_string(),
            is_redeemed: false,
        };
        let golden = serde_json::json!({
            "NonceGenerated": {
                "c_nonce": "test-c-nonce",
                "is_redeemed": false
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn nonce_redeemed() {
        let event = NonceEvent::NonceRedeemed {
            c_nonce: "test-c-nonce".to_string(),
            is_redeemed: true,
        };
        let golden = serde_json::json!({
            "NonceRedeemed": {
                "c_nonce": "test-c-nonce",
                "is_redeemed": true
            }
        });
        assert_round_trip_and_golden(event, golden);
    }
}
