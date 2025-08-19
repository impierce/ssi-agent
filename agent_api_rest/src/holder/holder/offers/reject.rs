use crate::handlers::command_handler;
use agent_holder::{offer::command::OfferCommand, state::HolderState};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::StatusCode;

#[axum_macros::debug_handler]
pub(crate) async fn reject(
    State(state): State<HolderState>,
    Path(received_offer_id): Path<String>,
) -> Result<Response, ApiError> {
    let command = OfferCommand::RejectCredentialOffer {
        received_offer_id: received_offer_id.clone(),
    };

    // Remove the Credential Offer from the state.
    command_handler(&received_offer_id, &state.command.offer, command).await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
