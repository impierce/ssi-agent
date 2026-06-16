use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("Catalog name already exists: {0}")]
    DuplicateName(String),

    #[error("Templates not found: {0}")]
    TemplatesNotFound(String),

    #[error("Template already in Catalog: {0}")]
    TemplateAlreadyInCatalog(String),

    #[error("Template not in Catalog: {0}")]
    TemplateNotInCatalog(String),

    #[error("Missing required field: {0}")]
    MissingField(String),
}
