// Endpoint handlers
pub mod connections;
pub mod documents;
pub mod profiles;
pub mod services;
pub mod well_known;

pub mod error;

use agent_identity::state::IdentityState;
use axum::{
    routing::{get, post},
    Router,
};
use connections::{get_connection, get_connections, post_connections};
use documents::{get_document, get_documents};
use services::{linked_vp::linked_vp, service, services};
use std::sync::Arc;
use well_known::{did::did, did_configuration::did_configuration};

use crate::{
    v0::identity::profiles::{get_profile, patch_profile},
    API_VERSION,
};

pub fn router(identity_state: Arc<IdentityState>) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/connections", get(get_connections).post(post_connections))
                .route("/connections/{connection_id}", get(get_connection))
                .route("/documents", get(get_documents))
                .route("/documents/{document_id}", get(get_document))
                .route("/profile", get(get_profile).patch(patch_profile))
                .route("/services", get(services))
                .route("/services/{service_id}", get(service))
                .route("/services/linked-vp", post(linked_vp)),
        )
        .route("/.well-known/did.json", get(did))
        .route("/.well-known/did-configuration.json", get(did_configuration))
        .with_state(identity_state)
}
