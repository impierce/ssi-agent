use oauth_tsl::status_list::StatusType;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StatusListCommand {
    AddIndex { status: StatusType },
    UpdateIndex {id: Url, index: usize, status: StatusType },
    CreateToken { id: Url },
}