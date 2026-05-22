use chrono::{DateTime, Utc};
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Display)]
pub enum ReissuanceEvent {
    ReissuanceCreated {
        reissuance_id: String,
        original_credential_id: String,
        new_credential_id: String,
        offer_id: String,
        credential_configuration_id: String,
        reason: Option<String>,
        trigger_type: Option<String>,
        triggered_by: Option<String>,
        status_action: Option<String>,
        created_at: DateTime<Utc>,
    },
}

impl DomainEvent for ReissuanceEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
