use agent_issuance::{offer::command::OfferCommand, state::IssuanceState};
use agent_shared::handlers::command_handler;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::info;
use url::Url;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendOfferEndpointRequest {
    pub offer_id: String,
    pub target_url: Url,
}

#[axum_macros::debug_handler]
pub(crate) async fn send(State(state): State<IssuanceState>, Json(payload): Json<serde_json::Value>) -> Response {
    info!("Request Body: {}", payload);

    let Ok(SendOfferEndpointRequest { offer_id, target_url }) = serde_json::from_value(payload) else {
        return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
    };

    let command = OfferCommand::SendCredentialOffer {
        offer_id: offer_id.clone(),
        target_url,
    };

    // Send the Credential Offer to the `target_url`.
    match command_handler(&offer_id, &state.command.offer, command).await {
        Ok(_) => StatusCode::OK.into_response(),
        // TODO: add better Error responses. This needs to be done properly in all endpoints once
        // https://github.com/impierce/openid4vc/issues/78 is fixed.
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
