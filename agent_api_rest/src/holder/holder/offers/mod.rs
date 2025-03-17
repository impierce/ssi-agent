pub mod accept;
pub mod reject;

use crate::handlers::query_handler;
use agent_holder::state::HolderState;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;

#[axum_macros::debug_handler]
pub(crate) async fn offers(State(state): State<HolderState>) -> Result<Response, ApiError> {
    query_handler("all_received_offers", &state.query.all_received_offers)
        .await?
        .map(|all_received_offers_view| {
            let all_received_offers = all_received_offers_view
                .received_offers
                .into_values()
                .collect::<Vec<_>>();

            (StatusCode::OK, Json(all_received_offers)).into_response()
        })
        .ok_or_else(|| {
            ApiError::builder(StatusCode::CONFLICT)
                .title("Optimistic Lock Error")
                .message("An optimistic lock error occurred while committing an aggregate.")
                .finish()
        })
}

#[axum_macros::debug_handler]
pub(crate) async fn offer(
    State(state): State<HolderState>,
    Path(received_offer_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&received_offer_id, &state.query.received_offer)
        .await?
        .map(|received_offer_view| (StatusCode::OK, Json(received_offer_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
