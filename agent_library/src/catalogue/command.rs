use crate::catalogue::aggregate::{CatalogueDisplay, CatalogueVisibility};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CatalogueCommand {
    CreateCatalogue {
        catalogue_id: String,
        display: CatalogueDisplay,
        visibility: CatalogueVisibility,
    },
    UpdateDisplay {
        catalogue_id: String,
        display: CatalogueDisplay,
    },
    UpdateVisibility {
        catalogue_id: String,
        visibility: CatalogueVisibility,
    },
    AddTemplateId {
        catalogue_id: String,
        template_id: String,
    },
    RemoveTemplateId {
        catalogue_id: String,
        template_id: String,
    },
    DeleteCatalogue {
        catalogue_id: String,
    },
}
