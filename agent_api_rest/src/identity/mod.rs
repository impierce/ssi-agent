pub mod services;
pub mod well_known;

use agent_identity::state::IdentityState;
use axum::{
    routing::{get, post},
    Router,
};
use services::linked_vp;
use well_known::{did::did, did_configuration::did_configuration};

use crate::API_VERSION;

pub fn router(identity_state: IdentityState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new().route("/services/linked-vp/:presentation_id", post(linked_vp)),
        )
        .route("/.well-known/did.json", get(did))
        .route("/.well-known/did-configuration.json", get(did_configuration))
        .with_state(identity_state)
}
