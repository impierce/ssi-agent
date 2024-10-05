use super::ConnectionView;
use crate::connection::views::Connection;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllConnectionsView {
    #[serde(flatten)]
    pub connections: HashMap<String, ConnectionView>,
}

impl View<Connection> for AllConnectionsView {
    fn update(&mut self, event: &EventEnvelope<Connection>) {
        self.connections
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
