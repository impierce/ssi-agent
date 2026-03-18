use identity_core::common::Url;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ConnectionCommand {
    AddConnection { connection_id: String, url: Url },
    SyncConnection { connection_id: String },
    AcceptConnectionChanges { connection_id: String },
    RemoveConnection { connection_id: String },
}
