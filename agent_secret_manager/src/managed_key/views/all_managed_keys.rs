use super::ManagedKeyView;
use crate::managed_key::aggregate::ManagedKey;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllManagedKeysView {
    #[serde(flatten)]
    pub managed_keys: HashMap<String, ManagedKeyView>,
}

impl View<ManagedKey> for AllManagedKeysView {
    fn update(&mut self, event: &EventEnvelope<ManagedKey>) {
        self.managed_keys
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
