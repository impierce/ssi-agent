use super::ConnectionView;
use crate::connection::event::ConnectionEvent;
use crate::connection::views::Connection;
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllConnectionsView {
    #[serde(flatten)]
    pub connections: IndexMap<String, ConnectionView>,
}

impl View<Connection> for AllConnectionsView {
    fn update(&mut self, event: &EventEnvelope<Connection>) {
        if let ConnectionEvent::ConnectionRemoved { connection_id } = &event.payload {
            self.connections.shift_remove(connection_id);
            return;
        }
        self.connections
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
