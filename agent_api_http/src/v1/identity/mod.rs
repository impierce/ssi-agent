pub mod keys;

use crate::v1::identity::keys::{generate_key, get_keys, remove_key, rename_key_alias, set_signing_key};
use crate::API_VERSION;
use agent_identity::state::IdentityState;
use axum::{
    routing::{get, post},
    Router,
};

use std::sync::Arc;

pub fn router(identity_state: Arc<IdentityState>) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/keys/generate-new-key", post(generate_key))
                .route("/keys/remove-key", post(remove_key))
                .route("/keys/rename-key-alias", post(rename_key_alias))
                .route("/keys/set-signing-key", post(set_signing_key))
                .route("/keys/get-keys", get(get_keys)),
        )
        .with_state(identity_state)
}
