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
            TemplateError::InvalidSchemaPropertiesAttributes(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Schema Properties Attributes")
                .type_url(type_url("templates#invalid-schema-properties-attributes"))
                .source(self)
                .finish(),
            TemplateError::NonRemovablePropertyViolation(_) => ApiError::builder(StatusCode::CONFLICT)
                .title("Non-removable Property Violation")
                .type_url(type_url("templates#non-removable-property-violation"))
                .source(self)
                .finish(),
            TemplateError::DisallowedOpenBadgesProperties(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Disallowed OpenBadges Schema Properties")
                .type_url(type_url("templates#disallowed-open-badges-properties"))
                .source(self)
                .finish(),
            TemplateError::MissingRequiredOpenBadgesProperties(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Missing Required OpenBadges Schema Properties")
                .type_url(type_url("templates#missing-required-open-badges-properties"))
                .source(self)
                .finish(),
            TemplateError::InvalidRequiredPropertyType(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Required Property Type")
                .type_url(type_url("templates#invalid-required-property-type"))
                .source(self)
                .finish(),
            TemplateError::MissingTitle => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Missing Title")
                .type_url(type_url("templates#missing-title"))
                .source(self)
                .finish(),
        }
    }
}
