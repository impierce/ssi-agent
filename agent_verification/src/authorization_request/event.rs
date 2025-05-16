use crate::generic_oid4vc::GenericAuthorizationRequest;
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, strum::Display)]
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
    SIOPv2AuthorizationResponseVerified {
        id_token: String,
        state: Option<String>,
    },
    OID4VPAuthorizationResponseVerified {
        vp_token: String,
        state: Option<String>,
    },
}

impl DomainEvent for AuthorizationRequestEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
