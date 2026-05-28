use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("Catalog name already exists: {0}")]
    DuplicateName(String),

    #[error("Template not found: {0}")]
    TemplateNotFound(String),

    #[error("Template already in Catalog: {0}")]
    TemplateAlreadyInCatalog(String),

    #[error("Template not in Catalog: {0}")]
    TemplateNotInCatalog(String),
}
