use cqrs_es::DomainEvent;
use identity_credential::credential::Jwt;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum PresentationEvent {
    PresentationCreated {
        presentation_id: String,
        signed_presentation: Jwt,
    },
}

impl DomainEvent for PresentationEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
