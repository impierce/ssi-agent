use super::Client;
use super::ClientView;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllClientsView {
    #[serde(flatten)]
    pub clients: HashMap<String, ClientView>,
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
