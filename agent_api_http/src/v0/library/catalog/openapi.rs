use crate::v0::library::catalog::{
    __path_add_templates_to_catalog, __path_create_catalog, __path_delete_catalog, __path_make_catalog_private,
    __path_make_catalog_public, __path_remove_templates_from_catalog, __path_change_catalog_appearance,
};

use crate::v0::library::catalog::queries::{
    get_all_catalogs::__path_get_all_catalogs, get_catalog_by_id::__path_get_catalog_by_id,
};

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(create_catalog, delete_catalog, make_catalog_public, make_catalog_private, add_templates_to_catalog, remove_templates_from_catalog, change_catalog_appearance, get_all_catalogs, get_catalog_by_id),
    tags(
        (name = "Catalog", description = "Create and manage catalogs to organize and share your templates.")
    )
)]
pub struct CatalogsApi;
