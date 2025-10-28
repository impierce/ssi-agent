pub mod error;
pub mod templates;

use agent_library::state::LibraryState;
use axum::{routing::get, Router};

use crate::{
    library::templates::{get_template, get_templates, patch_template, post_templates},
    API_VERSION,
};

pub fn router(library_state: LibraryState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/templates", get(get_templates).post(post_templates))
                .route("/templates/{template_id}", get(get_template).patch(patch_template)),
        )
        .with_state(library_state)
}
