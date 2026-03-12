use oauth_tsl::status_list::StatusType;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StatusListCommand {
    AddIndex { status: StatusType }, // TODO: add an optional id field here in the future. Currently the status lists will simply be filled "chronologically". However, there is a case for keeping certain status lists for certain credentials/purposes, which would then need to be able to be queried by id.
    UpdateIndex { id: Url, index: usize, status: StatusType },
}
