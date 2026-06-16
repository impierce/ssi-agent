use crate::catalog::aggregate::{CatalogDisplay, CatalogVisibility};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CatalogCommand {
    CreateCatalog {
        catalog_id: String,
        display: CatalogDisplay,
        visibility: CatalogVisibility,
    },
    UpdateDisplay {
        catalog_id: String,
        display: CatalogDisplay,
    },
    UpdateVisibility {
        catalog_id: String,
        visibility: CatalogVisibility,
    },
    AddTemplateIds {
        catalog_id: String,
        template_ids: Vec<String>,
    },
    RemoveTemplateId {
        catalog_id: String,
        template_id: String,
    },
    DeleteCatalog {
        catalog_id: String,
    },
}
