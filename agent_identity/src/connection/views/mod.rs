pub mod all_connections;

use super::event::ConnectionEvent;
use crate::connection::aggregate::{Connection, ConnectionProperties, DisplayProperties};
use cqrs_es::{EventEnvelope, View};
use identity_core::common::Url;
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConnectionView {
    pub connection_id: String,
    pub domain: Option<Url>,
    pub dids: Vec<DIDUrl>,
    pub display: Option<DisplayProperties>,
    pub pending_changes: Option<ConnectionProperties>,
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
            } => {
                self.connection_id.clone_from(connection_id);
                self.display.clone_from(display);
                self.domain.clone_from(domain);
                self.dids.clone_from(dids);
            }
            ConnectionSynced { pending_changes } => {
                self.pending_changes = pending_changes.clone();
            }
            ConnectionChangesAccepted {
                connection_id,
                display,
                domain,
                dids,
            } => {
                self.connection_id.clone_from(connection_id);
                self.display.clone_from(display);
                self.domain.clone_from(domain);
                self.dids.clone_from(dids);
                self.pending_changes = None;
            }
            ConnectionRemoved { connection_id: _ } => {}
        }
    }
}
