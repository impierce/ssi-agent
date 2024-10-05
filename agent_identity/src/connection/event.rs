use cqrs_es::DomainEvent;
use identity_core::common::Url;
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ConnectionEvent {
    ConnectionAdded {
        connection_id: String,
        domain: Option<Url>,
        dids: Vec<DIDUrl>,
        credential_offer_endpoint: Option<Url>,
    },
    DomainAdded {
        connection_id: String,
        domain: Url,
    },
    DidAdded {
        connection_id: String,
        did: DIDUrl,
    },
}

impl DomainEvent for ConnectionEvent {
    fn event_type(&self) -> String {
        use ConnectionEvent::*;

        let event_type: &str = match self {
            ConnectionAdded { .. } => "ConnectionAdded",
            DomainAdded { .. } => "DomainAdded",
            DidAdded { .. } => "DidAdded",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
