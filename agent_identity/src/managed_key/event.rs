use cqrs_es::DomainEvent;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, Serialize, Derivative, Display)]
#[derivative(PartialEq)]
pub enum ManagedKeyEvent {}

impl DomainEvent for ManagedKeyEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
