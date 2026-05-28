pub mod view_all_catalogs;
use super::event::CatalogEvent;
use crate::catalog::aggregate::Catalog;
use chrono::Utc;
use cqrs_es::{EventEnvelope, View};

pub type CatalogView = Catalog;

impl View<Catalog> for Catalog {
    fn update(&mut self, event: &EventEnvelope<Catalog>) {
        use CatalogEvent::*;

        self.modified_at = Utc::now();

        match &event.payload {
            CatalogCreated {
                id,
                display,
                visibility,
            } => {
                self.catalog_id.clone_from(id);
                self.display.clone_from(display);
                self.visibility.clone_from(visibility);
            }
            CatalogDisplayUpdated { id: _, display } => {
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
            CatalogDeleted { id: _ } => {
                self.is_deleted = true;
            }
        }
    }
}
