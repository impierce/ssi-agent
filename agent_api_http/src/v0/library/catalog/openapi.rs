use crate::v0::library::catalog::{
    __path_add_templates, __path_create_catalog, __path_delete_catalog, __path_remove_templates, __path_update_display,
    __path_update_visibility,
};

use crate::v0::library::catalog::queries::{
    get_catalog::__path_get_catalog, get_all_catalogs::__path_get_all_catalogs,
};

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(create_catalog, delete_catalog, add_templates, remove_templates, update_display, update_visibility, get_all_catalogs, get_catalog),
    tags(
        (name = "Library", description = "Manage your own templates, browse and import external templates."),
        (name = "Catalogs", description = "Create and manage Catalogs to organize and share your templates.")
    )
)]
pub struct CatalogsApi;
