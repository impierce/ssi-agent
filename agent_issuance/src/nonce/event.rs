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

        let round_tripped: NonceEvent =
            serde_json::from_value(serialized).expect("serialized event should deserialize");
        assert_eq!(round_tripped, event, "round-trip through JSON changed the event");

        let from_golden: NonceEvent = serde_json::from_value(golden).expect("golden fixture should deserialize");
        assert_eq!(
            from_golden, event,
            "golden fixture no longer deserializes into the expected event"
        );
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

/// Demonstrates the version-bump migration path from `docs/event-versioning.md` on a real event
/// enum: a hand-built legacy (version `"1"`) [`SerializedEvent`](cqrs_es::persist::SerializedEvent)
/// whose payload predates a (hypothetical) breaking change only deserializes into the current
/// `NonceEvent` after the matching upcaster has rewritten it.
#[cfg(test)]
mod upcaster_tests {
    use super::*;
    use cqrs_es::persist::{EventUpcaster, SemanticVersionEventUpcaster, SerializedEvent};
    use serde_json::json;

    /// A `NonceGenerated` event as it would sit in the event store if the version-`"1"` payload
    /// had lacked the `is_redeemed` field.
    fn legacy_nonce_generated() -> SerializedEvent {
        SerializedEvent::new(
            "nonce-1".to_string(),
            1,
            "nonce".to_string(),
            "NonceGenerated".to_string(),
            "1".to_string(),
            json!({ "NonceGenerated": { "c_nonce": "legacy-c-nonce" } }),
            json!({}),
        )
    }

    /// Demo upcaster for the hypothetical `"1"` → `"2"` bump: defaults the missing `is_redeemed`
    /// field to `false`.
    fn demo_upcaster() -> SemanticVersionEventUpcaster {
        SemanticVersionEventUpcaster::new(
            "NonceGenerated",
            "2",
            Box::new(|mut payload| {
                if let Some(inner) = payload
                    .get_mut("NonceGenerated")
                    .and_then(|inner| inner.as_object_mut())
                {
                    inner.insert("is_redeemed".to_string(), json!(false));
                }
                payload
            }),
        )
    }

    #[test]
    fn the_legacy_payload_does_not_deserialize_without_upcasting() {
        serde_json::from_value::<NonceEvent>(legacy_nonce_generated().payload)
            .expect_err("the legacy payload lacks `is_redeemed` and must be rejected by the current enum");
    }

    #[test]
    fn the_upcasted_legacy_event_deserializes_into_the_current_enum() {
        let upcaster = demo_upcaster();
        let legacy_event = legacy_nonce_generated();

        assert!(upcaster.can_upcast(&legacy_event.event_type, &legacy_event.event_version));

        let upcasted = upcaster.upcast(legacy_event);
        // `SemanticVersionEventUpcaster` stamps its full parsed version: `"2"` parses as `2.0.0`.
        // The exact string is irrelevant downstream since versions are only compared after parsing.
        assert_eq!(upcasted.event_version, "2.0.0");

        let event: NonceEvent =
            serde_json::from_value(upcasted.payload).expect("the upcasted payload matches the current schema");
        assert_eq!(
            event,
            NonceEvent::NonceGenerated {
                c_nonce: "legacy-c-nonce".to_string(),
                is_redeemed: false,
            }
        );
    }

    #[test]
    fn events_at_or_above_the_target_version_or_of_other_types_are_not_upcasted() {
        let upcaster = demo_upcaster();
        assert!(!upcaster.can_upcast("NonceGenerated", "2"));
        assert!(!upcaster.can_upcast("NonceGenerated", "3"));
        assert!(!upcaster.can_upcast("NonceRedeemed", "1"));
    }
}
