// TODO: further refactor the API's folder structure to reflect the API's routes.
#[allow(clippy::module_inception)]
pub mod holder;
pub mod openid4vci;

use crate::holder::holder::{
    credentials::credentials,
    offers::{accept::accept, reject::reject, *},
};
use crate::API_VERSION;
use agent_holder::state::HolderState;
use axum::routing::get;
use axum::{routing::post, Router};

pub fn router(holder_state: HolderState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/holder/credentials", get(credentials))
                .route("/holder/offers", get(offers))
                .route("/holder/offers/:offer_id/accept", post(accept))
                .route("/holder/offers/:offer_id/reject", post(reject)),
        )
        .route("/openid4vci/offers", get(openid4vci::offers))
        .with_state(holder_state)
}
