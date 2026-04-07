use cqrs_es::DomainEvent;
use oauth_tsl::status_list::{StatusList, StatusType};
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, Display, PartialEq, Serialize)]
pub enum StatusListEvent {
    StatusListCreated {
        id: String,
        status_list: StatusList,
        used_indices: Vec<usize>,
    },
    IndexAdded {
        id: String,
        status_list: StatusList,
        used_indices: Vec<usize>,
        index: usize,       // Metadata, not used in the event
        status: StatusType, // Metadata
    },
    IndexUpdated {
        id: String,
        status_list: StatusList,
        index: usize,       // Metadata
        status: StatusType, // Metadata
    },
}

impl DomainEvent for StatusListEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
