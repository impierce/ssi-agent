use crate::handlers::{command_handler, request_actor};
use agent_issuance::{offer::aggregate::DeliveryMethod, offer::command::OfferCommand, state::IssuanceState};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Extension, Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Actor;
use std::sync::Arc;
use url::Url;

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailOfferEndpointRequest {
    pub offer_id: String,
    pub recipient_email: String,
}

/// Send offer to individual
///
/// Sends a credential offer to an individual's email.
#[utoipa::path(
    post,
    path = "/offers/send-offer-to-individual",
    operation_id = "send_offer_to_individual",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Offer sent successfully")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn individual_offer(
    State(state): State<Arc<IssuanceState>>,
    actor: Option<Extension<Option<Actor>>>,
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
    command_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &offer_id,
        &state.command.offer,
        command,
    )
    .await?;

    Ok(StatusCode::OK.into_response())
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetUrlOfferEndpointRequest {
    pub offer_id: String,
    pub target_url: Url,
}

/// Send offer to organization
///
/// Sends a credential offer to an organization's URL.
#[utoipa::path(
    post,
    path = "/offers/send-offer-to-organization",
    operation_id = "send_offer_to_organization",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Offer sent successfully")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn organization_offer(
    State(state): State<Arc<IssuanceState>>,
    actor: Option<Extension<Option<Actor>>>,
    Json(TargetUrlOfferEndpointRequest { offer_id, target_url }): Json<TargetUrlOfferEndpointRequest>,
) -> Result<Response, ApiError> {
    let command = OfferCommand::SendCredentialOffer {
        offer_id: offer_id.clone(),
        delivery_method: DeliveryMethod::TargetUrl { target_url },
    };

    // Send the offer to the organizational url.
    command_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &offer_id,
        &state.command.offer,
        command,
    )
    .await?;

    Ok(StatusCode::OK.into_response())
}
