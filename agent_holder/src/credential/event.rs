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
