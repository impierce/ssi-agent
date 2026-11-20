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
    #[serde(default)]
    pub deleted: bool,
}

impl View<Key> for KeyView {
    fn update(&mut self, event: &EventEnvelope<Key>) {
        match &event.payload {
            KeyEvent::KeyCreated { reference } | KeyEvent::KeyAdopted { reference } => {
                self.reference = Some(reference.clone());
            }
            KeyEvent::KeyDeleted => self.deleted = true,
        }
    }
}

/// Every key this instance created or adopted and didn't delete, by key ID, in the order they were recorded.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AllKeysView {
    #[serde(flatten)]
    pub keys: IndexMap<String, KeyReference>,
}

impl View<Key> for AllKeysView {
    fn update(&mut self, event: &EventEnvelope<Key>) {
        match &event.payload {
            KeyEvent::KeyCreated { reference } | KeyEvent::KeyAdopted { reference } => {
                self.keys.insert(event.aggregate_id.clone(), reference.clone());
            }
            KeyEvent::KeyDeleted => {
                self.keys.shift_remove(&event.aggregate_id);
            }
        }
    }
}
