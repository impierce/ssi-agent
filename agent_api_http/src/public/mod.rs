use agent_library::state::LibraryState;
use axum::{routing::get, Router};
use std::sync::Arc;

pub mod openapi;
pub mod sponsoring_configuration;
pub mod templates;

/// Build the router for the unauthenticated `/public` endpoints.
///
/// Routes that need a bounded context's state are only mounted when that state is available.
pub fn router(library_state: Option<Arc<LibraryState>>) -> Router {
    Router::new().nest(
        "/public",
        Router::new()
            .route(
                "/sponsoring-configuration",
                get(sponsoring_configuration::sponsoring_configuration),
            )
            .merge(
                library_state
                    .map(|library_state| {
                        Router::new()
                            .route("/templates", get(templates::get_public_templates))
                            .with_state(library_state)
                    })
                    .unwrap_or_default(),
            ),
    )
}
