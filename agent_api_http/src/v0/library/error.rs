use crate::error::IntoApiErrorExt;
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
                .source(self)
                .finish(),
        }
    }
}
