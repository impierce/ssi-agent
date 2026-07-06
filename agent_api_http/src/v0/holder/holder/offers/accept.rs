use crate::extractors::RequestActor;
use crate::handlers::{command_handler, query_handler};
use agent_holder::{
    credential::command::CredentialCommand,
    offer::{
        aggregate::{Offer, OfferCredential},
        command::OfferCommand,
        queries::ReceivedOfferView,
    },
    state::HolderState,
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use std::sync::Arc;

/// Accept a credential offer
///
/// Accepts a credential offered to your organization by its ID.
#[utoipa::path(
    post,
    path = "/holder/offers/{offer_id}/accept",
    operation_id = "accept_credential_offer",
    tags = ["Identity", "Holder"],
    responses(
        (status = 201, description = "Credential offer accepted successfully", body = Offer),
        (status = 404, description = "Credential offer not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn accept(
    State(state): State<Arc<HolderState>>,
    RequestActor(actor): RequestActor,
    Path(received_offer_id): Path<String>,
) -> Result<Response, ApiError> {
    // TODO: General note that also applies to other endpoints: currently we are using Application Layer logic in the
    // REST API. This is not ideal and should be changed. The REST API should only be responsible for handling HTTP
    // Requests and Responses.
    // Furthermore, the Application Layer (not implemented yet) should be kept very thin as well. See: https://github.com/impierce/ssi-agent/issues/114

    // Check if the Credential Offer exists.
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &received_offer_id,
        &state.query.received_offer,
    )
    .await?
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))?;

    let command = OfferCommand::AcceptCredentialOffer {
        received_offer_id: received_offer_id.clone(),
    };

    // Accept the Credential Offer
    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &received_offer_id,
        &state.command.offer,
        command,
    )
    .await?;

    let command = OfferCommand::SendCredentialRequest {
        received_offer_id: received_offer_id.clone(),
    };

    // Send the Credential Request
    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &received_offer_id,
        &state.command.offer,
        command,
    )
    .await?;

    let credentials = match query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &received_offer_id,
        &state.query.received_offer,
    )
    .await?
    {
        Some(ReceivedOfferView { credentials, .. }) => credentials,
        // TODO: this *should* be an impossible error, what should we return here?
        _ => return Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)),
    };

    for OfferCredential {
        holder_credential_id,
        credential,
    } in credentials
    {
        let command = CredentialCommand::AddCredential {
            holder_credential_id: holder_credential_id.clone(),
            received_offer_id: Some(received_offer_id.clone()),
            credential,
        };

        // Add the Credential to the state.
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &holder_credential_id,
            &state.command.credential,
            command,
        )
        .await?;
    }

    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &received_offer_id,
        &state.query.received_offer,
    )
    .await?
    .map(|received_offer_view| (StatusCode::CREATED, Json(received_offer_view)).into_response())
    // TODO: this *should* be an impossible error, what should we return here?
    .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}
