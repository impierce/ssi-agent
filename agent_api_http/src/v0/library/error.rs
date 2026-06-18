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
            CatalogError::DuplicateName(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Duplicate Catalog Name")
                .source(self)
                .finish(),
            CatalogError::TemplatesNotFound(_) => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Template Not Found")
                .source(self)
                .finish(),
            CatalogError::TemplateAlreadyInCatalog(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Template Already In Catalog")
                .source(self)
                .finish(),
            CatalogError::TemplateNotInCatalog(_) => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Template Not In Catalog")
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
        }
    }
}
