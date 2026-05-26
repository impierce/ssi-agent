use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogueError {
    #[error("Catalogue name already exists: {0}")]
    DuplicateName(String),

    #[error("Template not found: {0}")]
    TemplateNotFound(String),

    #[error("Template already in catalogue: {0}")]
    TemplateAlreadyInCatalogue(String),

    #[error("Template not in catalogue: {0}")]
    TemplateNotInCatalogue(String),
}
