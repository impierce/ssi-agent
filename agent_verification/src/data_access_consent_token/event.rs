use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, strum::Display)]
pub enum DataAccessConsentTokenEvent {
    DataAccessConsentTokenStored { id: String, token: String },
    DataAccessConsentTokenResolved { id: String, called_endpoint: String },
}

impl DomainEvent for DataAccessConsentTokenEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
