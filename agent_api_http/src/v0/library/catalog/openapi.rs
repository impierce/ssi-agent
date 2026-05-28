use crate::v0::library::catalog::{
    __path_add_template, __path_create_catalog, __path_delete_catalog, __path_remove_template,
    __path_update_display, __path_update_visibility,
};

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(create_catalog, delete_catalog, add_template, remove_template, update_display, update_visibility),
    tags(
        (name = "Library", description = "Manage your own templates, browse and import external templates."),
        (name = "Catalogs", description = "Create and manage Catalogs to organize and share your templates.")
    )
)]
pub struct CatalogsApi;
