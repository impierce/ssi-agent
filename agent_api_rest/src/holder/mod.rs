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
use holder::{
    credentials::credential,
    presentations::{get_presentations, post_presentations, presentation, presentation_signed::presentation_signed},
};

pub fn router(holder_state: HolderState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/holder/credentials", get(credentials))
                .route("/holder/credentials/:credential_id", get(credential))
                .route("/holder/presentations", get(get_presentations).post(post_presentations))
                .route("/holder/presentations/:presentation_id", get(presentation))
                .route(
                    "/holder/presentations/:presentation_id/signed",
                    get(presentation_signed),
                )
                .route("/holder/offers", get(offers))
                .route("/holder/offers/:offer_id", get(offer))
                .route("/holder/offers/:offer_id/accept", post(accept))
                .route("/holder/offers/:offer_id/reject", post(reject)),
        )
        .route("/openid4vci/offers", get(openid4vci::offers))
        .with_state(holder_state)
}
