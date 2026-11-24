use crate::key::aggregate::KeyReference;
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Display)]
pub enum KeyEvent {
    /// This instance created the key in its provider.
    KeyCreated { reference: KeyReference },
    /// The key already existed in its provider, and this instance recorded it.
    KeyAdopted { reference: KeyReference },
    /// The key was deleted from its provider, and its label freed.
    KeyDeleted,
}

impl KeyEvent {
    /// The key this event records, or `None` when it ends the key's life.
    #[must_use]
    pub fn recorded(&self) -> Option<&KeyReference> {
        match self {
            Self::KeyCreated { reference } | Self::KeyAdopted { reference } => Some(reference),
            Self::KeyDeleted => None,
        }
    }
}

impl DomainEvent for KeyEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
