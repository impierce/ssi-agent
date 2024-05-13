use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ConnectionEvent {
    SIOPv2AuthorizationResponseVerified { id_token: String },
    OID4VPAuthorizationResponseVerified { vp_token: String },
}

impl DomainEvent for ConnectionEvent {
    fn event_type(&self) -> String {
        use ConnectionEvent::*;

        let event_type: &str = match self {
            SIOPv2AuthorizationResponseVerified { .. } => "SIOPv2AuthorizationResponseVerified",
            OID4VPAuthorizationResponseVerified { .. } => "OID4VPAuthorizationResponseVerified",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
