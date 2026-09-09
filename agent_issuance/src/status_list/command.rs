use oauth_tsl::status_list::StatusType;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::CommandOperation;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StatusListCommand {
    CreateStatusList { id: String },
    AddIndex { status: StatusType },
    UpdateIndex { index: usize, status: StatusType },
}

impl CommandOperation for StatusListCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateStatusList { .. } => "issuance.status_lists.create",
            Self::AddIndex { .. } => "issuance.status_lists.indices.add",
            Self::UpdateIndex { .. } => "issuance.status_lists.indices.update",
        }
    }
}
