pub mod all_connections;

use super::event::ConnectionEvent;
use crate::connection::aggregate::{Connection, ConnectionDisplayProperties, PendingChanges, Validation};
use chrono::{DateTime, Utc};
use cqrs_es::{EventEnvelope, View};
use identity_core::common::Url;
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
#[schema(as = Connection)]
pub struct ConnectionView {
    #[serde(rename = "id")]
    pub connection_id: String,
    pub url: Option<Url>,
    #[schema(value_type = Vec<String>)]
    pub dids: Vec<DIDUrl>,
    pub validations: Vec<Validation>,
    pub display: Option<ConnectionDisplayProperties>,
    pub pending_changes: Option<PendingChanges>,
    pub first_interacted_at: Option<DateTime<Utc>>,
    pub last_interacted_at: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub deleted: bool,
}

impl View<Connection> for ConnectionView {
    fn update(&mut self, event: &EventEnvelope<Connection>) {
        use ConnectionEvent::*;

        match &event.payload {
            ConnectionAdded {
                connection_id,
                display,
                url,
                dids,
                validations,
                first_interacted_at,
                last_interacted_at,
            } => {
                self.connection_id.clone_from(connection_id);
                self.display.clone_from(display);
                self.url = Some(url.clone());
                self.dids.clone_from(dids);
                self.validations.clone_from(validations);
                self.first_interacted_at = *first_interacted_at;
                self.last_interacted_at = *last_interacted_at;
            }
            ConnectionSynced {
                connection_id,
                pending_changes,
                last_interacted_at,
                validations,
            } => {
                self.connection_id = connection_id.clone();
                self.pending_changes = pending_changes.clone();
                self.last_interacted_at = *last_interacted_at;
                self.validations.clone_from(validations);
            }
            ConnectionChangesAccepted {
                connection_id,
                display,
                dids,
                last_interacted_at,
                pending_changes,
            } => {
                self.connection_id.clone_from(connection_id);
                self.display.clone_from(display);
                self.dids.clone_from(dids);
                self.last_interacted_at = *last_interacted_at;
                self.pending_changes.clone_from(pending_changes);
            }
            ConnectionRemoved { connection_id: _ } => {
                self.deleted = true;
            }
        }
    }
}
