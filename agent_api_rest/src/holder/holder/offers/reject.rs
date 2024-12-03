use agent_holder::{offer::command::OfferCommand, state::HolderState};
use agent_shared::handlers::command_handler;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use hyper::StatusCode;

#[axum_macros::debug_handler]
pub(crate) async fn reject(State(state): State<HolderState>, Path(received_offer_id): Path<String>) -> Response {
    let command = OfferCommand::RejectCredentialOffer {
        received_offer_id: received_offer_id.clone(),
    };

    // Remove the Credential Offer from the state.
    if command_handler(&received_offer_id, &state.command.offer, command)
        .await
        .is_err()
    {
        // TODO: add better Error responses. This needs to be done properly in all endpoints once
        // https://github.com/impierce/openid4vc/issues/78 is fixed.
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // TODO: What do we return here?
    StatusCode::OK.into_response()
}
