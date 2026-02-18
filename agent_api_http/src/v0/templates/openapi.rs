use crate::v0::templates::{
    __path_create_template, __path_duplicate_template, __path_get_template, __path_get_templates,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(create_template, duplicate_template, get_templates, get_template),
    tags(
        (name = "library", description = "Manage your own templates, browse and import external templates."),
        (name = "templates", description = "Create and manage templates which provide the structure for credentials to be issued.")
    )
)]
pub(crate) struct TemplatesApi;
