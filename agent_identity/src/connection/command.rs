use identity_core::common::Url;
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ConnectionCommand {
    AddConnection { connection_id: String, url: Url },
    SyncConnection { connection_id: String },
    AcceptConnectionChanges { connection_id: String },
    RemoveConnection { connection_id: String },
}

impl CommandOperation for ConnectionCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::AddConnection { .. } => "identity.connections.add",
            Self::SyncConnection { .. } => "identity.connections.sync",
            Self::AcceptConnectionChanges { .. } => "identity.connections.changes.accept",
            Self::RemoveConnection { .. } => "identity.connections.remove",
        }
    }
}
