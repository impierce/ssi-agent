use chrono::{DateTime, Utc};
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Display)]
pub enum RefreshCapabilityEvent {
    RefreshCapabilityCreated {
        refresh_reference: String,
        credential_id: String,
        created_at: DateTime<Utc>,
    },
    RefreshCapabilityDisabled,
}

impl DomainEvent for RefreshCapabilityEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
