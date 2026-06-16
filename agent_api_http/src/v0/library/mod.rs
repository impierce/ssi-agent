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
        queries::{get_all_catalogs::get_all_catalogs, get_catalog::get_catalog},
        {add_templates, create_catalog, delete_catalog, remove_template, update_display, update_visibility},
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
                .route("/templates/{template_id}", get(get_template))
                .route("/templates/get-all-templates", get(get_templates))
                .route("/templates/create-template", post(create_template))
                .route("/templates/delete-template", post(delete_template))
                .route("/templates/update-template", post(update_template))
                .route("/templates/duplicate-template", post(duplicate_template))
                // catalog Routes
                .route("/catalog/create-catalog", post(create_catalog))
                .route("/catalog/delete-catalog", post(delete_catalog))
                .route("/catalog/add-templates", post(add_templates))
                .route("/catalog/remove-template", post(remove_template))
                .route("/catalog/update-display", post(update_display))
                .route("/catalog/update-visibility", post(update_visibility))
                .route("/catalog/get-all-catalogs", get(get_all_catalogs))
                .route("/catalog/{catalog_id}", get(get_catalog)),
        )
        .with_state(library_state)
}
