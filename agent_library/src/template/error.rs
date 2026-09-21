use thiserror::Error;

#[derive(Error, Debug)]
pub enum TemplateError {
    #[error("Invalid JSON Schema: {0}")]
    InvalidSchema(String),
    #[error("Invalid status transition: {0}")]
    InvalidStatusTransition(String),
    #[error("Invalid status on create: only Draft or Published are allowed")]
    InvalidStatusOnCreate,
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
    #[error("Invalid type or format for OpenBadges 3.0 schema properties: {0}")]
    InvalidOpenBadgesPropertyType(String),
    #[error("A title is required when creating or updating a template")]
    MissingTitle,
    #[error("Archived templates are immutable except for status changes")]
    ArchivedTemplateImmutable,
    #[error("Deleted templates are terminal and cannot be changed")]
    DeletedTemplateTerminal,
    #[error("Published templates must be archived before they can be deleted")]
    ArchiveBeforeDeleteRequired,
    #[error("Invalid expiration value: {0}")]
    InvalidExpiration(String),
    #[error("Invalid type: {0}")]
    InvalidType(String),
    #[error("schemaPropertiesAttributes are not allowed for W3C VC 1.1 templates")]
    SchemaPropertiesAttributesNotAllowed,
    #[error("Duplicate schemaPropertiesAttributes key after trimming: `{0}`")]
    DuplicateSchemaPropertiesAttributeKey(String),
    #[error("A template must not be in \"Draft\" stage when making it public")]
    DraftTemplateCannotBePublic,
    #[error("No Source Template found with id: `{0}`")]
    SourceTemplateNotFound(String),
    #[error("The `id` field is required to update a template.")]
    TemplateIdMissing,
    #[error("No Template found with id: `{0}`")]
    TemplateNotFound(String),
}
