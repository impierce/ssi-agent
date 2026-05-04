use thiserror::Error;

#[derive(Error, Debug)]
pub enum TemplateError {
    #[error("Invalid JSON Schema: {0}")]
    InvalidSchema(String),
    #[error("Invalid schema_properties_attributes key(s): {0}")]
    InvalidSchemaPropertiesAttributes(String),
    #[error("Cannot remove immutable schema properties: {0}")]
    NonRemovablePropertyViolation(String),
    #[error("Disallowed OpenBadges 3.0 schema properties: {0}")]
    DisallowedOpenBadgesProperties(String),
    #[error("Missing required OpenBadges 3.0 schema properties: {0}")]
    MissingRequiredOpenBadgesProperties(String),
    #[error("Invalid type for required OpenBadges 3.0 schema properties: {0}")]
    InvalidRequiredPropertyType(String),
    #[error("A title is required when creating or updating a template")]
    MissingTitle,
    #[error("{0}")]
    ImmutableDataModel(String),
    #[error("{0}")]
    ImmutableHolderType(String),
}
