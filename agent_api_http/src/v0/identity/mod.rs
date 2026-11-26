// Endpoint handlers
pub mod connections;
pub mod documents;
pub mod profiles;
pub mod services;
pub mod well_known;

pub mod error;
pub mod openapi;

use agent_identity::{document::key_source::KeySource, state::IdentityState};
use axum::{
    routing::{get, post},
    Router,
};
use connections::{
    accept_connection_changes, get_connection, get_connections, post_connection, remove_connection, sync_connection,
};
use documents::{
    get_document, get_documents, get_signing_keys_endpoint, post_add_key_to_document, post_deselect_signing_key,
    post_remove_key_from_document, post_select_signing_key,
};
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

/// The routes that change and list the keys of the DID documents, adding keys from `key_source`. Only mounted with the
/// Key Management System, since without its keys there is nothing to add or select.
pub fn document_keys_router((identity_state, key_source): (Arc<IdentityState>, Arc<dyn KeySource>)) -> Router {
    Router::new().nest(
        API_VERSION,
        Router::new()
            .route("/add-key-to-document", post(post_add_key_to_document))
            .with_state((identity_state.clone(), key_source))
            .merge(
                Router::new()
                    .route("/remove-key-from-document", post(post_remove_key_from_document))
                    .route("/select-signing-key", post(post_select_signing_key))
                    .route("/deselect-signing-key", post(post_deselect_signing_key))
                    .route("/get-signing-keys", get(get_signing_keys_endpoint))
                    .with_state(identity_state),
            ),
    )
}

pub fn well_known_router(identity_state: Arc<IdentityState>) -> Router {
    Router::new()
        .route("/.well-known/did.json", get(did))
        .route("/.well-known/did-configuration.json", get(did_configuration))
        .with_state(identity_state)
}
