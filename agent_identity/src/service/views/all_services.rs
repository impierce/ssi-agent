use super::ServiceView;
use crate::service::aggregate::Service;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllServicesView {
    #[serde(flatten)]
    pub services: HashMap<String, ServiceView>,
}

impl View<Service> for AllServicesView {
    fn update(&mut self, event: &EventEnvelope<Service>) {
        self.services
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
