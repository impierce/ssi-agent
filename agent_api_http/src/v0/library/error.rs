use crate::error::{type_url, IntoApiErrorExt};
use agent_library::template::error::TemplateError;
use http_api_problem::ApiError;
use hyper::StatusCode;

impl IntoApiErrorExt for TemplateError {
    fn into_api_error(self) -> ApiError {
        match self {
            TemplateError::InvalidSchema(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid JSON Schema")
                .source(self)
                .finish(),
            TemplateError::InvalidStatusTransition(_) => ApiError::builder(StatusCode::CONFLICT)
                .title("Invalid Status Transition")
                .type_url(type_url("library#invalid-status-transition"))
                .source(self)
                .finish(),
            TemplateError::InvalidSchemaPropertiesAttributes(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Schema Properties Attributes")
                .type_url(type_url("library#invalid-schema-properties-attributes"))
                .source(self)
                .finish(),
            TemplateError::NonRemovablePropertyViolation(_) => ApiError::builder(StatusCode::CONFLICT)
                .title("Non-removable Property Violation")
                .type_url(type_url("library#non-removable-property-violation"))
                .source(self)
                .finish(),
            TemplateError::DisallowedOpenBadgesProperties(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Disallowed OpenBadges Schema Properties")
                .type_url(type_url("library#disallowed-open-badges-properties"))
                .source(self)
                .finish(),
            TemplateError::MissingRequiredOpenBadgesProperties(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Missing Required OpenBadges Schema Properties")
                .type_url(type_url("library#missing-required-open-badges-properties"))
                .source(self)
                .finish(),
            TemplateError::InvalidRequiredPropertyType(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Required Property Type")
                .type_url(type_url("library#invalid-required-property-type"))
                .source(self)
                .finish(),
            TemplateError::MissingTitle => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Missing Title")
                .type_url(type_url("library#missing-title"))
                .source(self)
                .finish(),
            TemplateError::ArchivedTemplateImmutable => ApiError::builder(StatusCode::CONFLICT)
                .title("Archived Template Immutable")
                .type_url(type_url("library#archived-template-immutable"))
                .source(self)
                .finish(),
            TemplateError::DeletedTemplateTerminal => ApiError::builder(StatusCode::CONFLICT)
                .title("Deleted Template Terminal")
                .type_url(type_url("library#deleted-template-terminal"))
                .source(self)
                .finish(),
            TemplateError::ArchiveBeforeDeleteRequired => ApiError::builder(StatusCode::CONFLICT)
                .title("Archive Before Delete Required")
                .type_url(type_url("library#archive-before-delete-required"))
                .source(self)
                .finish(),
            TemplateError::InvalidExpiration(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Expiration")
                .type_url(type_url("library#invalid-expiration"))
                .source(self)
                .finish(),
            TemplateError::InvalidType(_) => ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Invalid Type")
                .type_url(type_url("library#invalid-type"))
                .source(self)
                .finish(),
            TemplateError::InvalidStatusOnCreate => ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Invalid Status On Create")
                .type_url(type_url("library#invalid-status-on-create"))
                .source(self)
                .finish(),
            TemplateError::SchemaPropertiesAttributesNotAllowed => {
                ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                    .title("Schema Properties Attributes Not Allowed")
                    .type_url(type_url("library#schema-properties-attributes-not-allowed"))
                    .source(self)
                    .finish()
            }
            TemplateError::DuplicateSchemaPropertiesAttributeKey(_) => {
                ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                    .title("Duplicate Schema Properties Attribute Key")
                    .type_url(type_url("library#duplicate-schema-properties-attribute-key"))
                    .source(self)
                    .finish()
            }
        }
    }
}
