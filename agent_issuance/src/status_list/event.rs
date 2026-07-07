use cqrs_es::DomainEvent;
use oauth_tsl::status_list::{StatusList, StatusType};
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, Display, PartialEq, Serialize)]
pub enum StatusListEvent {
    StatusListCreated {
        id: String,
        status_list: StatusList,
        used_indices: Vec<usize>,
    },
    IndexAdded {
        id: String,
        status_list: StatusList,
        used_indices: Vec<usize>,
        index: usize,       // Metadata, not used in the event
        status: StatusType, // Metadata
    },
    IndexUpdated {
        id: String,
        status_list: StatusList,
        index: usize,       // Metadata
        status: StatusType, // Metadata
    },
}

impl DomainEvent for StatusListEvent {
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
    use oauth_tsl::status_list::Bits;

    /// Asserts that `event` serializes to exactly `golden`, that it round-trips losslessly
    /// through JSON, and that the golden fixture itself still deserializes into `event`.
    fn assert_round_trip_and_golden(event: StatusListEvent, golden: serde_json::Value) {
        let serialized = serde_json::to_value(&event).expect("event should serialize");
        assert_eq!(serialized, golden, "serialized event drifted from the golden fixture");

        let round_tripped: StatusListEvent =
            serde_json::from_value(serialized).expect("serialized event should deserialize");
        assert_eq!(round_tripped, event, "round-trip through JSON changed the event");

        let from_golden: StatusListEvent = serde_json::from_value(golden).expect("golden fixture should deserialize");
        assert_eq!(
            from_golden, event,
            "golden fixture no longer deserializes into the expected event"
        );
    }

    fn fixed_status_list() -> StatusList {
        StatusList {
            status_size: Bits::Two,
            status_list: vec![0, 1, 2, 3],
            aggregation_uri: None,
        }
    }

    #[test]
    fn status_list_created() {
        let event = StatusListEvent::StatusListCreated {
            id: "status-list-id".to_string(),
            status_list: fixed_status_list(),
            used_indices: vec![],
        };
        let golden = serde_json::json!({
            "StatusListCreated": {
                "id": "status-list-id",
                "status_list": { "bits": "Two", "lst": [0, 1, 2, 3] },
                "used_indices": []
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn index_added() {
        let event = StatusListEvent::IndexAdded {
            id: "status-list-id".to_string(),
            status_list: fixed_status_list(),
            used_indices: vec![5],
            index: 5,
            status: StatusType::VALID,
        };
        let golden = serde_json::json!({
            "IndexAdded": {
                "id": "status-list-id",
                "status_list": { "bits": "Two", "lst": [0, 1, 2, 3] },
                "used_indices": [5],
                "index": 5,
                "status": "VALID"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn index_updated() {
        let event = StatusListEvent::IndexUpdated {
            id: "status-list-id".to_string(),
            status_list: fixed_status_list(),
            index: 5,
            status: StatusType::INVALID,
        };
        let golden = serde_json::json!({
            "IndexUpdated": {
                "id": "status-list-id",
                "status_list": { "bits": "Two", "lst": [0, 1, 2, 3] },
                "index": 5,
                "status": "INVALID"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }
}
