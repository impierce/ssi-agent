use cqrs_es::DomainEvent;
use identity_core::common::Url;
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum ProfileEvent {
    ProfileCreated {
        profile_id: String,
        display_name: Option<String>,
        logo_uri: Option<Url>,
        provisioned: bool,
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
