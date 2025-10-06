pub mod error;
pub mod templates;

use agent_catalog::state::CatalogState;
use axum::{routing::get, Router};

use crate::{
    catalog::templates::{get_template, get_templates, post_templates},
    API_VERSION,
};

pub fn router(catalog_state: CatalogState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/templates", get(get_templates).post(post_templates))
                .route("/templates/{template_id}", get(get_template)),
        )
        .with_state(catalog_state)
}
