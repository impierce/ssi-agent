use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("Templates not found: {0}")]
    TemplatesNotFound(String),
    #[error("Template already in Catalog: {0}")]
    MissingField(String),
    #[error("Catalog not found: {0}")]
    CatalogNotFound(String),
    #[error("Duplicate templates found: {0}")]
    DuplicateTemplate(String),

    
}
