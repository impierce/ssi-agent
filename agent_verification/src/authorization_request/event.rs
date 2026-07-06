use crate::generic_oid4vc::GenericAuthorizationRequest;
use cqrs_es::DomainEvent;
use oid4vp::token::vp_token_validator::DecodedVpToken;
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
        validated: bool,
    },
    OID4VPAuthorizationResponseVerified {
        vp_token: DecodedVpToken,
        state: Option<String>,
        validated: bool,
    },
}

impl DomainEvent for AuthorizationRequestEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    // Integer schema version of this event payload. Bump on breaking change and add an upcaster (see docs/event-versioning.md).
    fn event_version(&self) -> String {
        "1".to_string()
    }
}

/// Upcasters migrating old persisted versions of these events to the current
/// schema version. See `docs/event-versioning.md`.
pub fn upcasters() -> Vec<Box<dyn cqrs_es::persist::EventUpcaster>> {
    vec![]
}
