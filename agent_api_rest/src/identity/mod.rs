pub mod services;
pub mod well_known;

use agent_identity::state::IdentityState;
use axum::{
    routing::{get, post},
    Router,
};
use services::{linked_vp::linked_vp, services};
use well_known::{did::did, did_configuration::did_configuration};

use crate::API_VERSION;

pub fn router(identity_state: IdentityState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/services", get(services))
                .route("/services/linked-vp", post(linked_vp)),
        )
        .route("/.well-known/did.json", get(did))
        .route("/.well-known/did-configuration.json", get(did_configuration))
        .with_state(identity_state)
}
