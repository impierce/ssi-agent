use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::credential::aggregate::Credential;
use crate::credential::value_object::Subject;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum CredentialEvent {
    CredentialFormatTemplateLoaded {
        credential_format_template: serde_json::Value,
    },
    SubjectCreated {
        subject: Subject,
    },
    UnsignedCredentialCreated {
        // subject_id: String,
        credential: Credential,
    },
}

impl DomainEvent for CredentialEvent {
    fn event_type(&self) -> String {
        use CredentialEvent::*;

        let event_type: &str = match self {
            CredentialFormatTemplateLoaded { .. } => "CredentialFormatTemplateLoaded",
            SubjectCreated { .. } => "SubjectCreated",
            UnsignedCredentialCreated { .. } => "UnsignedCredentialCreated",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
