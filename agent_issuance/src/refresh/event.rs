use crate::refresh::aggregate::Continuation;
use chrono::{DateTime, Utc};
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

/// Events of a [`RefreshCapability`](crate::refresh::aggregate::RefreshCapability). They never contain the refresh
/// token, and they are never published.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Display)]
pub enum RefreshCapabilityEvent {
    RefreshCapabilityCreated {
        credential_id: String,
        credential_configuration_id: String,
        created_at: DateTime<Utc>,
    },
    ContinuationRecorded {
        continuation: Continuation,
    },
    RefreshCapabilityRetired {
        accepted_credential_id: String,
    },
}

impl DomainEvent for RefreshCapabilityEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
