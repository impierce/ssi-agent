use crate::handlers::command_handler;
use agent_issuance::{offer::command::OfferCommand, state::IssuanceState};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use url::Url;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendOfferEndpointRequest {
    pub offer_id: String,
    pub target_url: Url,
}

#[axum_macros::debug_handler]
pub(crate) async fn send(
    State(state): State<Arc<IssuanceState>>,
    Json(SendOfferEndpointRequest { offer_id, target_url }): Json<SendOfferEndpointRequest>,
) -> Result<Response, ApiError> {
    let command = OfferCommand::SendCredentialOffer {
        offer_id: offer_id.clone(),
        target_url,
    };

    // Send the Credential Offer to the `target_url`.
    command_handler(&offer_id, &state.command.offer, command).await?;

    Ok(StatusCode::OK.into_response())
}
