use axum::{routing::get, Router};

pub mod sponsoring_configuration;

pub fn router() -> Router {
    Router::new().nest(
        "/public",
        Router::new().route(
            "/sponsoring-configuration",
            get(sponsoring_configuration::sponsoring_configuration),
        ),
    )
}
