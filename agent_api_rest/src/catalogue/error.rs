use crate::error::IntoApiErrorExt;
use agent_catalogue::template::error::TemplateError;
use http_api_problem::ApiError;

impl IntoApiErrorExt for TemplateError {
    fn into_api_error(self) -> ApiError {
        match self {}
    }
}
