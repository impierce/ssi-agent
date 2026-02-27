// use crate::connection::aggregate::DisplayProperties;
use identity_core::common::Url;
// use identity_did::DIDUrl;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ConnectionCommand {
    AddConnection {
        connection_id: String,
        // display: Option<DisplayProperties>,
        domain: Option<Url>,
        // dids: Vec<DIDUrl>,
        credential_offer_endpoint: Option<Url>,
    },
    SyncConnection {
        connection_id: String,
    },
    RemoveConnection {
        connection_id: String,
    },
}
