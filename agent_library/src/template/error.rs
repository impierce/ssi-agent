use thiserror::Error;

#[derive(Error, Debug)]
pub enum TemplateError {
    #[error("Invalid JSON Schema: {0}")]
    InvalidSchema(String),
}
