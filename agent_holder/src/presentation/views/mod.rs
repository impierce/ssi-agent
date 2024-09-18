pub mod all_presentations;

use super::aggregate::Presentation;
use cqrs_es::{EventEnvelope, View};

pub type PresentationView = Presentation;

impl View<Presentation> for Presentation {
    fn update(&mut self, event: &EventEnvelope<Presentation>) {
        use crate::presentation::event::PresentationEvent::*;

        match &event.payload {
            PresentationCreated {
                presentation_id,
                signed_presentation,
            } => {
                self.presentation_id.clone_from(presentation_id);
                self.signed_presentation.replace(signed_presentation.clone());
            }
        }
    }
}
