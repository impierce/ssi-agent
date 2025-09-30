pub mod error;
pub mod templates;

use agent_catalogue::state::CatalogueState;
use axum::{routing::get, Router};

use crate::{
    catalogue::templates::{get_template, get_templates, post_templates},
    API_VERSION,
};

pub fn router(catalogue_state: CatalogueState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/templates", get(get_templates).post(post_templates))
                .route("/templates/{template_id}", get(get_template)),
        )
        .with_state(catalogue_state)
}
