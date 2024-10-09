use agent_holder::{
    credential::command::CredentialCommand,
    offer::{aggregate::OfferCredential, command::OfferCommand, queries::ReceivedOfferView},
    state::HolderState,
};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;

#[axum_macros::debug_handler]
pub(crate) async fn accept(State(state): State<HolderState>, Path(received_offer_id): Path<String>) -> Response {
    // TODO: General note that also applies to other endpoints: currently we are using Application Layer logic in the
    // REST API. This is not ideal and should be changed. The REST API should only be responsible for handling HTTP
    // Requests and Responses.
    // Furthermore, the Application Layer (not implemented yet) should be kept very thin as well. See: https://github.com/impierce/ssi-agent/issues/114

    // Accept the Credential Offer if it exists
    match query_handler(&received_offer_id, &state.query.received_offer).await {
        Ok(Some(ReceivedOfferView { .. })) => {
            let command = OfferCommand::AcceptCredentialOffer {
                received_offer_id: received_offer_id.clone(),
            };

            if command_handler(&received_offer_id, &state.command.offer, command)
                .await
                .is_err()
            {
                // TODO: add better Error responses. This needs to be done properly in all endpoints once
                // https://github.com/impierce/openid4vc/issues/78 is fixed.
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }

    let command = OfferCommand::SendCredentialRequest {
        received_offer_id: received_offer_id.clone(),
    };

    // Send the Credential Request
    if command_handler(&received_offer_id, &state.command.offer, command)
        .await
        .is_err()
    {
        // TODO: add better Error responses. This needs to be done properly in all endpoints once
        // https://github.com/impierce/openid4vc/issues/78 is fixed.
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let credentials = match query_handler(&received_offer_id, &state.query.received_offer).await {
        Ok(Some(ReceivedOfferView { credentials, .. })) => credentials,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    for OfferCredential {
        holder_credential_id,
        credential,
    } in credentials
    {
        let command = CredentialCommand::AddCredential {
            holder_credential_id: holder_credential_id.clone(),
            received_offer_id: received_offer_id.clone(),
            credential,
        };

        // Add the Credential to the state.
        if command_handler(&holder_credential_id, &state.command.credential, command)
            .await
            .is_err()
        {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    match query_handler(&received_offer_id, &state.query.received_offer).await {
        Ok(Some(received_offer_view)) => (StatusCode::OK, Json(received_offer_view)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
