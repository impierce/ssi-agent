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
            TemplateError::InvalidSchemaPropertiesAttributes(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Schema Properties Attributes")
                .type_url(type_url("templates#invalid-schema-properties-attributes"))
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
