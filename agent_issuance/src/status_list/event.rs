use cqrs_es::DomainEvent;
use oauth_tsl::status_list::{StatusList, StatusType};
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

#[derive(Clone, Debug, Deserialize, Display, PartialEq, Serialize)]
pub enum StatusListEvent {
    IndexAdded { id: Url, status_list: StatusList, index: usize, status: StatusType },
    IndexUpdated { id: Url, index: usize, status: StatusType },
    TokenCreated { id: Url },
}

impl DomainEvent for StatusListEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
