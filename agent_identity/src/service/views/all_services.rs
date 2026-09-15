use super::ServiceView;
use crate::service::aggregate::Service;
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllServicesView {
    #[serde(flatten)]
    pub services: IndexMap<String, ServiceView>,
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
