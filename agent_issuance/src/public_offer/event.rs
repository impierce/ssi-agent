use chrono::{DateTime, Utc};
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

/// Domain events representing public offer lifecycle changes
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum PublicOfferEvent {
    Created {
        offer_id: String,
        template_id: String,
        created_at: DateTime<Utc>,
    },
    TakenOffline {
        offer_id: String,
    },
    TakenOnline {
        offer_id: String,
    },
    Deleted {
        offer_id: String,
    },
}

impl DomainEvent for PublicOfferEvent {
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
    fn assert_round_trip_and_golden(event: PublicOfferEvent, golden: serde_json::Value) {
        let serialized = serde_json::to_value(&event).expect("event should serialize");
        assert_eq!(serialized, golden, "serialized event drifted from the golden fixture");

        let round_tripped: PublicOfferEvent =
            serde_json::from_value(serialized).expect("serialized event should deserialize");
        assert_eq!(round_tripped, event, "round-trip through JSON changed the event");

        let from_golden: PublicOfferEvent =
            serde_json::from_value(golden).expect("golden fixture should deserialize");
        assert_eq!(from_golden, event, "golden fixture no longer deserializes into the expected event");
    }

    fn fixed_created_at() -> DateTime<Utc> {
        "2010-01-01T00:00:00Z".parse().unwrap()
    }

    #[test]
    fn created() {
        let event = PublicOfferEvent::Created {
            offer_id: "public-offer-id".to_string(),
            template_id: "template-id".to_string(),
            created_at: fixed_created_at(),
        };
        let golden = serde_json::json!({
            "Created": {
                "offer_id": "public-offer-id",
                "template_id": "template-id",
                "created_at": "2010-01-01T00:00:00Z"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn taken_offline() {
        let event = PublicOfferEvent::TakenOffline {
            offer_id: "public-offer-id".to_string(),
        };
        let golden = serde_json::json!({
            "TakenOffline": {
                "offer_id": "public-offer-id"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn taken_online() {
        let event = PublicOfferEvent::TakenOnline {
            offer_id: "public-offer-id".to_string(),
        };
        let golden = serde_json::json!({
            "TakenOnline": {
                "offer_id": "public-offer-id"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn deleted() {
        let event = PublicOfferEvent::Deleted {
            offer_id: "public-offer-id".to_string(),
        };
        let golden = serde_json::json!({
            "Deleted": {
                "offer_id": "public-offer-id"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }
}
