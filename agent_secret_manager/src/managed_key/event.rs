use crate::managed_key::aggregate::SigningAlgorithm;
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, Serialize, Display, PartialEq)]
pub enum ManagedKeyEvent {
    KeyGenerated {
        managed_key_id: String,
        key_id: String,
        alias: String,
        signing_algorithm: SigningAlgorithm,
    },
    KeyRemoved {
        managed_key_id: String,
    },
    KeyAliasUpdated {
        managed_key_id: String,
        new_alias: String,
    },
    SigningKeySet {
        managed_key_id: String,
    },
    SigningKeyUnset {
        managed_key_id: String,
    },
}

impl DomainEvent for ManagedKeyEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
