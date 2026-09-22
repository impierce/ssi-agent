use crate::catalog::aggregate::{CatalogDisplay, CatalogVisibility};
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

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

impl CommandOperation for CatalogCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateCatalog { .. } => "library.catalogs.create",
            Self::ChangeCatalogAppearance { .. } => "library.catalogs.appearance.update",
            Self::MakeCatalogPublic { .. } => "library.catalogs.visibility.make_public",
            Self::MakeCatalogPrivate { .. } => "library.catalogs.visibility.make_private",
            Self::AddTemplateIds { .. } => "library.catalogs.templates.add",
            Self::RemoveTemplateIds { .. } => "library.catalogs.templates.remove",
            Self::DeleteCatalog { .. } => "library.catalogs.delete",
        }
    }
}
