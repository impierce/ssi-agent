use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum PublicOfferError {
    #[error("Public offer already exists")]
    AlreadyExists,
    #[error("Public offer not found")]
    NotFound,
    #[error("Template not found")]
    TemplateNotFound,
    #[error("Template schema must only contain const-only leaf fields for public offers")]
    TemplateNotEligible,
}
