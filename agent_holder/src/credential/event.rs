use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

use super::aggregate::Data;
use super::signed::SignedCredential;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum CredentialEvent {
    CredentialAdded {
        holder_credential_id: String,
        received_offer_id: Option<String>,
        credential: SignedCredential,
        data: Data,
    },
}

impl DomainEvent for CredentialEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
