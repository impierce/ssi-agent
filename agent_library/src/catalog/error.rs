use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("Template not found: {0}")]
    TemplateNotFound(String),
    #[error("Catalog name is required")]
    MissingCatalogName(String),
    #[error("Catalog not found: {0}")]
    CatalogNotFound(String),
}
