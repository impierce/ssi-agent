pub mod authorization_requests;
pub mod relying_party;

use agent_verification::state::VerificationState;
use axum::routing::get;
use axum::{routing::post, Router};

use crate::verification::{
    authorization_requests::authorization_requests, authorization_requests::get_authorization_requests,
    relying_party::redirect::redirect, relying_party::request::request,
};
use crate::API_VERSION;

pub fn router(verification_state: VerificationState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/authorization_requests", post(authorization_requests))
                .route(
                    "/authorization_requests/:authorization_request_id",
                    get(get_authorization_requests),
                ),
        )
        .route("/request/:request_id", get(request))
        .route("/redirect", post(redirect))
        .with_state(verification_state)
}
