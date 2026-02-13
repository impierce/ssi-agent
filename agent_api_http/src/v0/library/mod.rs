// Endpoint handlers

pub mod error;

use agent_library::state::LibraryState;
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;

use crate::{
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
                .route("/templates/duplicate-template", post(duplicate_template)),
        )
        .with_state(library_state)
}
