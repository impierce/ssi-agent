use crate::connection::aggregate::DisplayProperties;
use cqrs_es::DomainEvent;
use identity_core::common::Url;
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum ConnectionEvent {
    ConnectionAdded {
        connection_id: String,
        display: Option<DisplayProperties>,
        domain: Option<Url>,
        dids: Vec<DIDUrl>,
        credential_offer_endpoint: Option<Url>,
    },
    ConnectionRemoved {
        connection_id: String,
    },
    ConnectionUpdated {
        connection_id: String,
        display: Option<DisplayProperties>,
        domain: Option<Url>,
        dids: Vec<DIDUrl>,
        credential_offer_endpoint: Option<Url>,
    },
}

impl DomainEvent for ConnectionEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
