use crate::handlers::command_handler;
use agent_issuance::{offer::aggregate::DeliveryMethod, offer::command::OfferCommand, state::IssuanceState};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendOfferEndpointRequest {
    pub offer_id: String,
    #[serde(flatten)]
    pub delivery_method: DeliveryMethod,
}

#[axum_macros::debug_handler]
pub(crate) async fn send(
    State(state): State<IssuanceState>,
    Json(SendOfferEndpointRequest {
        offer_id,
        delivery_method,
    }): Json<SendOfferEndpointRequest>,
) -> Result<Response, ApiError> {
    let command = OfferCommand::SendCredentialOffer {
        offer_id: offer_id.clone(),
        delivery_method,
    };

    // Send the Credential Offer to the `target_url` or to the recipient's email.
    command_handler(&offer_id, &state.command.offer, command).await?;

    Ok(StatusCode::OK.into_response())
}
