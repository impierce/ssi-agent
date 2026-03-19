use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::status_list::{aggregate::StatusListAggregate, views::StatusListView};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllStatusListsView {
    #[serde(flatten)]
    pub status_lists: HashMap<String, StatusListView>,
}

impl View<StatusListAggregate> for AllStatusListsView {
    fn update(&mut self, event: &EventEnvelope<StatusListAggregate>) {
        self.status_lists
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
