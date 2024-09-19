use thiserror::Error;

#[derive(Error, Debug)]
pub enum DocumentError {
    #[error("Error while producing DID document: {0}")]
    ProduceDocumentError(String),
    #[error("Missing document")]
    MissingDocumentError,
    #[error("Error while adding service: {0}")]
    AddServiceError(String),
}
