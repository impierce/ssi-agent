// Endpoint handlers

pub mod catalogue;
pub mod error;

use agent_library::state::LibraryState;
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;

use crate::{
    v0::library::catalogue::{
        queries::{get_all_catalogues::get_catalogues, get_catalogue::get_catalogue},
        {add_template, create_catalogue, delete_catalogue, remove_template, update_display, update_visibility},
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
                // Catalogue Routes
                .route("/catalogue/create-catalogue", post(create_catalogue))
                .route("/catalogue/delete-catalogue", post(delete_catalogue))
                .route("/catalogue/add-template", post(add_template))
                .route("/catalogue/remove-template", post(remove_template))
                .route("/catalogue/update-display", post(update_display))
                .route("/catalogue/update-visibility", post(update_visibility))
                .route("/catalogue/get-all-catalogues", get(get_catalogues))
                .route("/catalogue/{catalogue_id}", get(get_catalogue)),
        )
        .with_state(library_state)
}
