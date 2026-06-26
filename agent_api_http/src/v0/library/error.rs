use crate::error::{type_url, IntoApiErrorExt};
use agent_library::catalog::error::CatalogError;
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
            TemplateError::SchemaPropertiesAttributesNotAllowed => ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Schema Properties Attributes Not Allowed")
                .type_url(type_url("library#schema-properties-attributes-not-allowed"))
                .source(self)
                .finish(),
            TemplateError::DuplicateSchemaPropertiesAttributeKey(_) => {
                ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                    .title("Duplicate Schema Properties Attribute Key")
                    .type_url(type_url("library#duplicate-schema-properties-attribute-key"))
                    .source(self)
                    .finish()
            }
            TemplateError::DraftTemplateCannotBePublic => ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Draft Template Cannot Be Public")
                .type_url(type_url("library#draft-template-cannot-be-public"))
                .source(self)
                .finish(),
            TemplateError::SourceTemplateNotFound(ref id) => ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Source Template Not Found")
                .type_url(type_url("library#source-template-not-found"))
                .message(format!("No Source Template found with id: `{id}`"))
                .source(self)
                .finish(),
            TemplateError::TemplateIdMissing => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Template ID Missing")
                .type_url(type_url("library#template-id-missing"))
                .message("The `id` field is required to update a template.")
                .source(self)
                .finish(),
            TemplateError::TemplateNotFound(ref template_id) => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Template Not Found")
                .type_url(type_url("library#template-not-found"))
                .message(format!("No Template found with id: `{template_id}`"))
                .source(self)
                .finish(),
        }
    }
}

impl IntoApiErrorExt for CatalogError {
    fn into_api_error(self) -> ApiError {
        match self {
            CatalogError::TemplatesNotFound(_) => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Template Not Found")
                .source(self)
                .finish(),
            CatalogError::MissingField(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Missing Required Field")
                .source(self)
                .finish(),
            CatalogError::CatalogNotFound(_) => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Catalog Not Found")
                .source(self)
                .finish(),
            CatalogError::DuplicateTemplate(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Duplicate Template Found")
                .source(self)
                .finish(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::tests::into_json_value;
    use serde_json::json;

    macro_rules! assert_problem_details {
        ($error:expr, $expected:expr) => {
            assert_eq!(
                into_json_value($error.into_api_error().into_axum_response()).await,
                $expected,
            );
        };
    }

    #[tokio::test]
    async fn template_errors_successfully_convert_to_problem_details() {
        assert_problem_details!(
            TemplateError::InvalidSchema("bad schema".to_string()),
            json!({
                "type": "https://httpstatuses.com/400",
                "title": "Invalid JSON Schema",
                "status": 400,
                "detail": "Invalid JSON Schema: bad schema"
            })
        );

        assert_problem_details!(
            TemplateError::InvalidStatusTransition("draft -> draft".to_string()),
            json!({
                "type": type_url("library#invalid-status-transition"),
                "title": "Invalid Status Transition",
                "status": 409,
                "detail": "Invalid status transition: draft -> draft"
            })
        );

        assert_problem_details!(
            TemplateError::InvalidSchemaPropertiesAttributes("bad key".to_string()),
            json!({
                "type": type_url("library#invalid-schema-properties-attributes"),
                "title": "Invalid Schema Properties Attributes",
                "status": 400,
                "detail": "Invalid schema_properties_attributes key(s): bad key"
            })
        );

        assert_problem_details!(
            TemplateError::NonRemovablePropertyViolation("/name".to_string()),
            json!({
                "type": type_url("library#non-removable-property-violation"),
                "title": "Non-removable Property Violation",
                "status": 409,
                "detail": "Cannot remove immutable schema properties: /name"
            })
        );

        assert_problem_details!(
            TemplateError::DisallowedOpenBadgesProperties("achievement.foo".to_string()),
            json!({
                "type": type_url("library#disallowed-open-badges-properties"),
                "title": "Disallowed OpenBadges Schema Properties",
                "status": 400,
                "detail": "Disallowed OpenBadges 3.0 schema properties: achievement.foo"
            })
        );

        assert_problem_details!(
            TemplateError::MissingRequiredOpenBadgesProperties("/achievement/name".to_string()),
            json!({
                "type": type_url("library#missing-required-open-badges-properties"),
                "title": "Missing Required OpenBadges Schema Properties",
                "status": 400,
                "detail": "Missing required OpenBadges 3.0 schema properties: /achievement/name"
            })
        );

        assert_problem_details!(
            TemplateError::InvalidRequiredPropertyType("/achievement/name".to_string()),
            json!({
                "type": type_url("library#invalid-required-property-type"),
                "title": "Invalid Required Property Type",
                "status": 400,
                "detail": "Invalid type for required OpenBadges 3.0 schema properties: /achievement/name"
            })
        );

        assert_problem_details!(
            TemplateError::MissingTitle,
            json!({
                "type": type_url("library#missing-title"),
                "title": "Missing Title",
                "status": 400,
                "detail": "A title is required when creating or updating a template"
            })
        );

        assert_problem_details!(
            TemplateError::ArchivedTemplateImmutable,
            json!({
                "type": type_url("library#archived-template-immutable"),
                "title": "Archived Template Immutable",
                "status": 409,
                "detail": "Archived templates are immutable except for status changes"
            })
        );

        assert_problem_details!(
            TemplateError::DeletedTemplateTerminal,
            json!({
                "type": type_url("library#deleted-template-terminal"),
                "title": "Deleted Template Terminal",
                "status": 409,
                "detail": "Deleted templates are terminal and cannot be changed"
            })
        );

        assert_problem_details!(
            TemplateError::ArchiveBeforeDeleteRequired,
            json!({
                "type": type_url("library#archive-before-delete-required"),
                "title": "Archive Before Delete Required",
                "status": 409,
                "detail": "Published templates must be archived before they can be deleted"
            })
        );

        assert_problem_details!(
            TemplateError::InvalidExpiration("PXD".to_string()),
            json!({
                "type": type_url("library#invalid-expiration"),
                "title": "Invalid Expiration",
                "status": 400,
                "detail": "Invalid expiration value: PXD"
            })
        );

        assert_problem_details!(
            TemplateError::InvalidType("bad type".to_string()),
            json!({
                "type": type_url("library#invalid-type"),
                "title": "Invalid Type",
                "status": 422,
                "detail": "Invalid type: bad type"
            })
        );

        assert_problem_details!(
            TemplateError::InvalidStatusOnCreate,
            json!({
                "type": type_url("library#invalid-status-on-create"),
                "title": "Invalid Status On Create",
                "status": 422,
                "detail": "Invalid status on create: only Draft or Published are allowed"
            })
        );

        assert_problem_details!(
            TemplateError::SchemaPropertiesAttributesNotAllowed,
            json!({
                "type": type_url("library#schema-properties-attributes-not-allowed"),
                "title": "Schema Properties Attributes Not Allowed",
                "status": 422,
                "detail": "schemaPropertiesAttributes are not allowed for W3C VC 1.1 templates"
            })
        );

        assert_problem_details!(
            TemplateError::DuplicateSchemaPropertiesAttributeKey("/name".to_string()),
            json!({
                "type": type_url("library#duplicate-schema-properties-attribute-key"),
                "title": "Duplicate Schema Properties Attribute Key",
                "status": 422,
                "detail": "Duplicate schemaPropertiesAttributes key after trimming: `/name`"
            })
        );

        assert_problem_details!(
            TemplateError::DraftTemplateCannotBePublic,
            json!({
                "type": type_url("library#draft-template-cannot-be-public"),
                "title": "Draft Template Cannot Be Public",
                "status": 422,
                "detail": "A template must not be in \"Draft\" stage when making it public"
            })
        );

        assert_problem_details!(
            TemplateError::SourceTemplateNotFound("source-id".to_string()),
            json!({
                "type": type_url("library#source-template-not-found"),
                "title": "Source Template Not Found",
                "status": 422,
                "detail": "No Source Template found with id: `source-id`"
            })
        );

        assert_problem_details!(
            TemplateError::TemplateIdMissing,
            json!({
                "type": type_url("library#template-id-missing"),
                "title": "Template ID Missing",
                "status": 400,
                "detail": "The `id` field is required to update a template."
            })
        );

        assert_problem_details!(
            TemplateError::TemplateNotFound("missing-id".to_string()),
            json!({
                "type": type_url("library#template-not-found"),
                "title": "Template Not Found",
                "status": 404,
                "detail": "No Template found with id: `missing-id`"
            })
        );
    }
}

