use oauth_tsl::status_list::StatusType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StatusListCommand {
    CreateStatusList {
        id: String,
    },
    AddIndex {
        status: StatusType,
    },
    UpdateIndex {
        id: String, // Is this ID needed?
        index: usize,
        status: StatusType,
    },
}
