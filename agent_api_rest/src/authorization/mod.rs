pub mod authorization_server;

use crate::authorization::authorization_server::consent::{get_consent, post_consent};
use crate::API_VERSION;
use agent_authorization::state::AuthorizationState;
use agent_issuance::state::IssuanceState;
use authorization_server::{authorize::authorize, par::par, token::token};
use axum::routing::get;
use axum::{routing::post, Router};

pub fn router((authorization_state, issuance_state): (AuthorizationState, IssuanceState)) -> Router {
    Router::new()
        .nest(API_VERSION, Router::new())
        .route("/auth/consent", get(get_consent).post(post_consent))
        .route("/auth/par", post(par))
        .route("/auth/authorize", get(authorize))
        .with_state(authorization_state.clone())
        .route("/auth/token", post(token))
        // The `state` below only applies to the `/auth/token` endpoint where the Pre-Authorized Code flow still requires a shared state with the Issuance Bounded Context.
        .with_state((authorization_state, issuance_state))
}
