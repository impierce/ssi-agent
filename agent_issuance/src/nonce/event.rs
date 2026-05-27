use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, Display, PartialEq, Serialize)]
pub enum NonceEvent {
    NonceGenerated { c_nonce: String, is_redeemed: bool },
    NonceRedeemed { c_nonce: String, is_redeemed: bool },
}

impl DomainEvent for NonceEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
