use agent_shared::config::Logo;
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum ProfileEvent {
    ProfileCreated {
        profile_id: String,
        display_name: Option<String>,
        logo: Option<Logo>,
        provisioned: Option<bool>,
    },
}

impl DomainEvent for ProfileEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
