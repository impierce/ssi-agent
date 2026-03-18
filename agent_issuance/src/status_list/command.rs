use oauth_tsl::status_list::StatusType;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub enum StatusListCommand {
    CreateStatusList { id: String },
    AddIndex { status: StatusType },
    UpdateIndex { index: usize, status: StatusType },
}
