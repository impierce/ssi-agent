use crate::connection::aggregate::{ConnectionProperties, DisplayProperties};
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
        display: Option<DisplayProperties>,
        domain: Option<Url>,
        dids: Vec<DIDUrl>,
        first_interacted: Option<DateTime<Utc>>,
        last_interacted: Option<DateTime<Utc>>,
    },
    ConnectionRemoved {
        connection_id: String,
    },
    ConnectionSynced {
        pending_changes: Option<ConnectionProperties>,
        last_interacted: Option<DateTime<Utc>>,
    },
    ConnectionChangesAccepted {
        connection_id: String,
        display: Option<DisplayProperties>,
        dids: Vec<DIDUrl>,
        last_interacted: Option<DateTime<Utc>>,
        pending_changes: Option<ConnectionProperties>,
    },
}

impl DomainEvent for ConnectionEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
