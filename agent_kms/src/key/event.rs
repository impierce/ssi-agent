use crate::key::aggregate::KeyReference;
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Display)]
pub enum KeyEvent {
    KeyCreated { reference: KeyReference },
    KeyAdopted { reference: KeyReference },
    KeyDeleted,
}

impl DomainEvent for KeyEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
