use crate::key::{
    aggregate::{Key, KeyReference},
    event::KeyEvent,
};
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyView {
    pub reference: Option<KeyReference>,
}

impl View<Key> for KeyView {
    fn update(&mut self, event: &EventEnvelope<Key>) {
        match &event.payload {
            KeyEvent::KeyCreated { reference } => self.reference = Some(reference.clone()),
        }
    }
}

/// Every key this instance created, by key ID, in the order they were created.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AllKeysView {
    #[serde(flatten)]
    pub keys: IndexMap<String, KeyReference>,
}

impl View<Key> for AllKeysView {
    fn update(&mut self, event: &EventEnvelope<Key>) {
        match &event.payload {
            KeyEvent::KeyCreated { reference } => {
                self.keys.insert(event.aggregate_id.clone(), reference.clone());
            }
        }
    }
}
