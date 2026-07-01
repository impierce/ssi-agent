pub mod view_all_catalogs;
use super::event::CatalogEvent;
use crate::catalog::aggregate::{Catalog, CatalogDisplay, CatalogVisibility};
use chrono::DateTime;
use chrono::Utc;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Deserialize, Default, Serialize, ToSchema)]
#[schema(as = Catalog)]
pub struct CatalogView {
    #[serde(rename = "id")]
    pub catalog_id: String,
    pub display: CatalogDisplay,
    pub template_ids: Vec<String>,
    pub visibility: CatalogVisibility,
    pub modified_at: DateTime<Utc>,
    pub deleted: bool,
}

impl View<Catalog> for CatalogView {
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
            CatalogAppearanceChanged { id: _, display } => {
                self.display.clone_from(display);
            }
            CatalogMadePublic { id: _, visibility } => {
                self.visibility.clone_from(visibility);
            }
            CatalogMadePrivate { id: _, visibility } => {
                self.visibility.clone_from(visibility);
            }
            TemplateIdsAdded { id: _, template_ids } => {
                self.template_ids.extend(template_ids.iter().cloned());
            }
            TemplateIdsRemoved { id: _, template_ids } => {
                self.template_ids.retain(|id| !template_ids.contains(id));
            }
            CatalogDeleted { id: _ } => {
                self.deleted = true;
            }
        }
    }
}
