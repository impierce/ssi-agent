// Endpoint handlers

pub mod catalog;
pub mod error;

use agent_library::state::LibraryState;
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;

use crate::{
    v0::library::catalog::{
        queries::{get_all_catalogs::get_all_catalogs, get_catalog_by_id::get_catalog_by_id},
        {
            add_templates_to_catalog, create_catalog, delete_catalog, make_catalog_private, make_catalog_public, remove_templates_from_catalog,
            change_catalog_appearance,
        },
    },
    v0::templates::{
        create_template, delete_template, duplicate_template, get_template, get_templates, update_template,
    },
    API_VERSION,
};

pub fn router(library_state: Arc<LibraryState>) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/get-template-by-id/{id}", get(get_template))
                .route("/list-all-templates", get(get_templates))
                .route("/create-new-template", post(create_template))
                .route("/delete-template", post(delete_template))
                .route("/update-template", post(update_template))
                .route("/duplicate-template", post(duplicate_template))
                // Catalog Routes
                .route("/create-new-catalog", post(create_catalog))
                .route("/delete-catalog", post(delete_catalog))
                .route("/add-templates-to-catalog", post(add_templates_to_catalog))
                .route("/remove-templates-from-catalog", post(remove_templates_from_catalog))
                .route("/change-catalog-appearance", post(change_catalog_appearance))
                .route("/make-catalog-public", post(make_catalog_public))
                .route("/make-catalog-private", post(make_catalog_private))
                .route("/get-all-catalogs", get(get_all_catalogs))
                .route("/get-catalog-by-id/{catalog_id}", get(get_catalog_by_id)),
        )
        .with_state(library_state)
}
