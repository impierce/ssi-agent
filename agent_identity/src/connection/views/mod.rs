pub mod all_connections;

use super::event::ConnectionEvent;
use crate::connection::aggregate::{Connection, ConnectionDisplayProperties, PendingChanges};
use chrono::{DateTime, Utc};
use cqrs_es::{EventEnvelope, View};
use identity_core::common::Url;
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConnectionView {
    pub connection_id: String,
    pub domain: Option<Url>,
    pub dids: Vec<DIDUrl>,
    pub display: Option<ConnectionDisplayProperties>,
    pub pending_changes: Option<PendingChanges>,
    pub first_interacted: Option<DateTime<Utc>>,
    pub last_interacted: Option<DateTime<Utc>>,
}

impl View<Connection> for ConnectionView {
    fn update(&mut self, event: &EventEnvelope<Connection>) {
        use ConnectionEvent::*;

        match &event.payload {
            ConnectionAdded {
                connection_id,
                display,
                domain,
                dids,
                first_interacted,
                last_interacted,
            } => {
                self.connection_id.clone_from(connection_id);
                self.display.clone_from(display);
                self.domain.clone_from(domain);
                self.dids.clone_from(dids);
                self.first_interacted.clone_from(first_interacted);
                self.last_interacted.clone_from(last_interacted);
            }
            ConnectionSynced {
                pending_changes,
                last_interacted,
            } => {
                self.pending_changes = pending_changes.clone();
                self.last_interacted = last_interacted.clone();
            }
            ConnectionChangesAccepted {
                connection_id,
                display,
                dids,
                last_interacted,
                pending_changes,
            } => {
                self.connection_id.clone_from(connection_id);
                self.display.clone_from(display);
                self.dids.clone_from(dids);
                self.last_interacted.clone_from(last_interacted);
                self.pending_changes.clone_from(pending_changes);
            }
            ConnectionRemoved { connection_id: _ } => {
                *self = ConnectionView::default();
            }
        }
    }
}
