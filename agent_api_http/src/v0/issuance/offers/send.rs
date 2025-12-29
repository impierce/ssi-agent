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
use std::sync::Arc;
use url::Url;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailOfferEndpointRequest {
    pub offer_id: String,
    #[serde(flatten)]
    pub recipient_email: String,
}

#[axum_macros::debug_handler]
pub(crate) async fn email_offer(
    State(state): State<Arc<IssuanceState>>,
    Json(EmailOfferEndpointRequest {
        offer_id,
        recipient_email,
    }): Json<EmailOfferEndpointRequest>,
) -> Result<Response, ApiError> {
    let command = OfferCommand::SendCredentialOffer {
        offer_id: offer_id.clone(),
        delivery_method: DeliveryMethod::Email { recipient_email },
    };

    // Send the Credential Offer to the recipient's email.
    command_handler(&offer_id, &state.command.offer, command).await?;

    Ok(StatusCode::OK.into_response())
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetUrlOfferEndpointRequest {
    pub offer_id: String,
    #[serde(flatten)]
    pub target_url: Url,
}

#[axum_macros::debug_handler]
pub(crate) async fn organization_offer(
    State(state): State<Arc<IssuanceState>>,
    Json(TargetUrlOfferEndpointRequest { offer_id, target_url }): Json<TargetUrlOfferEndpointRequest>,
) -> Result<Response, ApiError> {
    let command = OfferCommand::SendCredentialOffer {
        offer_id: offer_id.clone(),
        delivery_method: DeliveryMethod::TargetUrl { target_url },
    };

    // Send the offer to the organizational url.
    command_handler(&offer_id, &state.command.offer, command).await?;

    Ok(StatusCode::OK.into_response())
}
