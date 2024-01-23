use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WalletEvent {
    AccountLoaded { alias: String, address: String },
}

impl DomainEvent for WalletEvent {
    fn event_type(&self) -> String {
        match self {
            WalletEvent::AccountLoaded { .. } => "AccountLoaded".to_string(),
        }
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
