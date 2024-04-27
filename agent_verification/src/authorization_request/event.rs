use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

use super::aggregate::GenericAuthorizationRequest;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum AuthorizationRequestEvent {
    AuthorizationRequestCreated {
        authorization_request: Box<GenericAuthorizationRequest>,
    },
    FormUrlEncodedAuthorizationRequestCreated {
        form_url_encoded_authorization_request: String,
    },
    AuthorizationRequestObjectSigned {
        signed_authorization_request_object: String,
    },
}

impl DomainEvent for AuthorizationRequestEvent {
    fn event_type(&self) -> String {
        use AuthorizationRequestEvent::*;

        let event_type: &str = match self {
            AuthorizationRequestCreated { .. } => "AuthorizationRequestCreated",
            FormUrlEncodedAuthorizationRequestCreated { .. } => "FormUrlEncodedAuthorizationRequestCreated",
            AuthorizationRequestObjectSigned { .. } => "AuthorizationRequestObjectSigned",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
