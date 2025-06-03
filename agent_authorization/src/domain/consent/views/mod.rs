pub mod all_consents;

use super::aggregate::Consent;
use cqrs_es::{EventEnvelope, View};

pub type ConsentView = Consent;

impl View<Consent> for Consent {
    fn update(&mut self, event: &EventEnvelope<Consent>) {
        match &event.payload {
            _ => todo!(),
        }
    }
}
