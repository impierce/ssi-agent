use super::Client;
use super::ClientView;
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllClientsView {
    #[serde(flatten)]
    pub clients: IndexMap<String, ClientView>,
}

impl View<Client> for AllClientsView {
    fn update(&mut self, event: &EventEnvelope<Client>) {
        self.clients
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
