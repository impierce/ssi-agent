use crate::error::{type_url, IntoApiErrorExt};
use agent_library::catalogue::error::CatalogueError;
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

impl IntoApiErrorExt for CatalogueError {
    fn into_api_error(self) -> ApiError {
        match self {
            CatalogueError::DuplicateName(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Duplicate Catalogue Name")
                .source(self)
                .finish(),
            CatalogueError::TemplateNotFound(_) => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Template Not Found")
                .source(self)
                .finish(),
            CatalogueError::TemplateAlreadyInCatalogue(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Template Already In Catalogue")
                .source(self)
                .finish(),
            CatalogueError::TemplateNotInCatalogue(_) => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Template Not In Catalogue")
                .source(self)
                .finish(),
        }
    }
}
