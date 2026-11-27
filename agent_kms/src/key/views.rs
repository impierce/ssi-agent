use crate::key::aggregate::{Key, KeyReference};
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::QueryOperation;

/// One key, by its id. `reference` is `None` when there is no such key or it has been deleted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyView {
    pub reference: Option<KeyReference>,
}

impl View<Key> for KeyView {
    fn update(&mut self, event: &EventEnvelope<Key>) {
        self.reference = event.payload.recorded().cloned();
    }
}

/// Every key that is recorded and not deleted, by id, in the order they were recorded.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AllKeysView {
    pub keys: IndexMap<String, KeyReference>,
}

impl AllKeysView {
    /// The id of the one view of all keys, as the CQRS component builders name it.
    pub const ID: &'static str = "all_keys";
}

impl View<Key> for AllKeysView {
    fn update(&mut self, event: &EventEnvelope<Key>) {
        match event.payload.recorded() {
            Some(reference) => {
                self.keys.insert(event.aggregate_id.clone(), reference.clone());
            }
            None => {
                self.keys.shift_remove(&event.aggregate_id);
            }
        }
    }
}

impl QueryOperation for AllKeysView {
    const OPERATION_NAME: &'static str = "kms.keys.list";
}
