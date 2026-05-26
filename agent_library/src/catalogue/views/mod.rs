pub mod view_all_catalogues;
use super::event::CatalogueEvent;
use crate::catalogue::aggregate::{Catalogue};
use cqrs_es::{EventEnvelope, View};

pub type CatalogueView = Catalogue;

impl View<Catalogue> for Catalogue{
    fn update(&mut self, event: &EventEnvelope<Catalogue>) {
        use CatalogueEvent::*;

        match &event.payload {
            CatalogueCreated {
                id,
                display,
                visibility,
            } => {
                self.catalogue_id.clone_from(id);
                self.display.clone_from(display);
                self.visibility.clone_from(visibility);
            }
            CatalogueDisplayUpdated { id: _, display } => {
                self.display.clone_from(display);
            }
            VisibilityUpdated { id: _, visibility } => {
                self.visibility.clone_from(visibility);
            }
            TemplateIdAdded { id: _, template_id } => {
                if !self.template_ids.contains(template_id) {
                    self.template_ids.push(template_id.clone());
                }
            }
            TemplateIdRemoved { id: _, template_id } => {
                self.template_ids.retain(|id| id != template_id);
            }
            CatalogueDeleted { id: _ } => {
                self.is_deleted = true; 
            }
        }
    }
}