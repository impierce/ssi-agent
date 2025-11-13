pub mod authorization_requests;
pub mod public_verification;
pub mod relying_party;
pub mod validate_domain_linkage;
pub mod validate_linked_verifiable_presentation;

pub mod error;

use agent_verification::state::VerificationState;
use authorization_requests::all_authorization_requests;
use axum::routing::get;
use axum::{routing::post, Router};

use crate::verification::{
    authorization_requests::authorization_request, authorization_requests::authorization_requests,
    public_verification::public_verification, relying_party::redirect::redirect, relying_party::request::request,
};
use crate::API_VERSION;

pub fn router(verification_state: VerificationState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route(
                    "/authorization_requests",
                    post(authorization_requests).get(all_authorization_requests),
                )
                .route(
                    "/authorization_requests/{authorization_request_id}",
                    get(authorization_request),
                )
                .route("/public-verification", get(public_verification)),
        )
        .route("/request/{request_id}", get(request))
        .route("/redirect", post(redirect))
        .with_state(verification_state)
}
