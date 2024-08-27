pub mod holder;
pub mod openid4vci;

use agent_holder::state::HolderState;
use axum::routing::get;
use axum::{routing::post, Router};

use crate::holder::holder::{
    credentials::credentials,
    offers::{accept::accept, reject::reject, *},
};
use crate::API_VERSION;

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
