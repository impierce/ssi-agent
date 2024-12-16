use cqrs_es::DomainEvent;
use identity_credential::credential::Jwt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum PresentationEvent {
    PresentationCreated {
        presentation_id: String,
        signed_presentation: Jwt,
    },
}

impl DomainEvent for PresentationEvent {
    fn event_type(&self) -> String {
        use PresentationEvent::*;

        let event_type: &str = match self {
            PresentationCreated { .. } => "PresentationCreated",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
