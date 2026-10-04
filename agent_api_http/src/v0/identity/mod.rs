// Endpoint handlers
pub mod connections;
pub mod documents;
pub mod profiles;
pub mod services;
pub mod well_known;

pub mod error;
pub mod openapi;

use agent_identity::state::IdentityState;
use axum::{
    routing::{get, post},
    Router,
};
use connections::{
    accept_connection_changes, get_connection, get_connections, post_connection, remove_connection, sync_connection,
};
use documents::{get_document, get_documents};
use services::{
    linked_domains::{add_linked_domains, remove_linked_domains, verify_linked_domains},
    linked_vp::{
        add_linked_verifiable_presentations, get_linked_verifiable_presentation_by_id,
        remove_linked_verifiable_presentations,
    },
    service, services,
};
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
                .route("/connections", get(get_connections).post(post_connection))
                .route("/connections/{connection_id}", get(get_connection))
                .route("/connections/sync-connection", post(sync_connection))
                .route("/connections/accept-pending-changes", post(accept_connection_changes))
                .route("/connections/remove-connection", post(remove_connection))
                .route("/documents", get(get_documents))
                .route("/documents/{document_id}", get(get_document))
                .route("/profile", get(get_profile).patch(patch_profile))
                .route("/services", get(services))
                .route("/services/{service_id}", get(service))
                .route(
                    "/add-linked-verifiable-presentations",
                    post(add_linked_verifiable_presentations),
                )
                .route("/add-linked-domains", post(add_linked_domains))
                .route("/remove-linked-domains", post(remove_linked_domains))
                .route("/verify-linked-domains", get(verify_linked_domains))
                .route(
                    "/remove-linked-verifiable-presentations",
                    post(remove_linked_verifiable_presentations),
                ),
        )
        .route(
            "/linked-verifiable-presentations/{presentation_id}",
            get(get_linked_verifiable_presentation_by_id),
        )
        .with_state(identity_state)
}

pub fn well_known_router(identity_state: Arc<IdentityState>) -> Router {
    Router::new()
        .route("/.well-known/did.json", get(did))
        .route("/.well-known/did-configuration.json", get(did_configuration))
        .with_state(identity_state)
}
