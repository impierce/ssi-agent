use crate::v0::library::catalogue::{
    __path_add_template, __path_create_catalogue, __path_delete_catalogue, __path_remove_template,
    __path_update_display, __path_update_visibility,
};

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(create_catalogue, delete_catalogue, add_template, remove_template, update_display, update_visibility),
    tags(
        (name = "Library", description = "Manage your own templates, browse and import external templates."),
        (name = "Catalogues", description = "Create and manage catalogues to organize and share your templates.")
    )
)]
pub struct TemplatesApi;
