pub mod authorization_server;

pub mod error;

use crate::authorization::authorization_server::login::{get_login, post_login};
use crate::API_VERSION;
use agent_authorization::state::AuthorizationState;
use agent_issuance::state::IssuanceState;
use authorization_server::{authorize::authorize, par::par, token::token};
use axum::routing::get;
use axum::{routing::post, Router};

pub fn router((authorization_state, issuance_state): (AuthorizationState, IssuanceState)) -> Router {
    Router::new()
        .nest(API_VERSION, Router::new())
        .route("/auth/login", get(get_login).post(post_login))
        .route("/auth/consent", post(par))
        .route("/auth/par", post(par))
        .route("/auth/authorize", get(authorize))
        .with_state(authorization_state.clone())
        .route("/auth/token", post(token))
        .with_state((authorization_state, issuance_state))
}
