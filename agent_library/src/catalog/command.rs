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
    RemoveTemplateIds {
        catalog_id: String,
        template_ids:Vec<String>,
    },
    DeleteCatalog {
        catalog_id: String,
    },
}
