pub mod keys;

use crate::v1::identity::keys::{generate_key, list_all, remove_key, rename_key_alias, set_signing_key};
use crate::API_VERSION;
use agent_identity::application::sagas::key_generation_saga::KeyGenerationSaga;
use agent_identity::application::sagas::key_removal_saga::KeyRemovalSaga;
use agent_identity::state::IdentityState;
use axum::{
    routing::{get, post},
    Router,
};

use std::sync::Arc;

#[derive(Clone)]
pub struct IdentityContext {
    pub state: Arc<IdentityState>,
    pub key_generation_saga: KeyGenerationSaga,
    pub key_removal_saga: KeyRemovalSaga,
}

pub fn router(context: IdentityContext) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/keys/generate-new-key", post(generate_key))
                .route("/keys/remove-key", post(remove_key))
                .route("/keys/rename-key-alias", post(rename_key_alias))
                .route("/keys/set-signing-key", post(set_signing_key))
                .route("/keys/list-all", get(list_all)),
        )
        .with_state(context)
}
