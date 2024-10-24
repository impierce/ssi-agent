use agent_holder::{offer::command::OfferCommand, state::HolderState};
use agent_shared::handlers::command_handler;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;
use oid4vci::credential_offer::CredentialOffer;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Deserialize, Serialize)]
pub struct Oid4vciOfferEndpointRequest {
    #[serde(flatten)]
    pub credential_offer: CredentialOffer,
}

#[axum_macros::debug_handler]
pub(crate) async fn offers(State(state): State<HolderState>, Json(payload): Json<serde_json::Value>) -> Response {
    info!("Request Body: {}", payload);

    let Ok(Oid4vciOfferEndpointRequest { credential_offer }) = serde_json::from_value(payload) else {
        return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
    };

    let offer_id = uuid::Uuid::new_v4().to_string();

    let command = OfferCommand::ReceiveCredentialOffer {
        offer_id: offer_id.clone(),
        credential_offer,
    };

    // Add the Credential Offer to the state.
    match command_handler(&offer_id, &state.command.offer, command).await {
        Ok(_) => StatusCode::OK.into_response(),
        // TODO: add better Error responses. This needs to be done properly in all endpoints once
        // https://github.com/impierce/openid4vc/issues/78 is fixed.
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
