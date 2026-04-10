use thiserror::Error;

#[derive(Error, Debug)]
pub enum TemplateError {
    #[error("Invalid JSON Schema: {0}")]
    InvalidSchema(String),
    #[error("Invalid schema_properties_attributes key(s): {0}")]
    InvalidSchemaPropertiesAttributes(String),
}
