use crate::connection::aggregate::{ConnectionDisplayProperties, PendingChanges, Validation};
use chrono::{DateTime, Utc};
use cqrs_es::DomainEvent;
use identity_core::common::Url;
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum ConnectionEvent {
    ConnectionAdded {
        connection_id: String,
        display: Option<ConnectionDisplayProperties>,
        url: Url,
        dids: Vec<DIDUrl>,
        first_interacted_at: Option<DateTime<Utc>>,
        last_interacted_at: Option<DateTime<Utc>>,
        validations: Vec<Validation>,
    },
    ConnectionRemoved {
        connection_id: String,
    },
    ConnectionSynced {
        connection_id: String,
        validations: Vec<Validation>,
        pending_changes: Option<PendingChanges>,
        last_interacted_at: Option<DateTime<Utc>>,
    },
    ConnectionChangesAccepted {
        connection_id: String,
        display: Option<ConnectionDisplayProperties>,
        dids: Vec<DIDUrl>,
        last_interacted_at: Option<DateTime<Utc>>,
        pending_changes: Option<PendingChanges>,
    },
}

impl DomainEvent for ConnectionEvent {
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
