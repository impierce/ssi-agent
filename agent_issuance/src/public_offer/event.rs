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
