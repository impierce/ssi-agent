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
    ChangeCatalogAppearance {
        catalog_id: String,
        display: CatalogDisplay,
    },
    MakeCatalogPublic {
        catalog_id: String,
    },
    MakeCatalogPrivate {
        catalog_id: String,
    },
    AddTemplateIds {
        catalog_id: String,
        template_ids: Vec<String>,
    },
    RemoveTemplateIds {
        catalog_id: String,
        template_ids: Vec<String>,
    },
    DeleteCatalog {
        catalog_id: String,
    },
}
