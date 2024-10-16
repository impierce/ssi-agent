use agent_holder::{
    credential::command::CredentialCommand,
    offer::{command::OfferCommand, queries::ReceivedOfferView},
    state::HolderState,
};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use hyper::StatusCode;

#[axum_macros::debug_handler]
pub(crate) async fn accept(State(state): State<HolderState>, Path(offer_id): Path<String>) -> Response {
    // TODO: General note that also applies to other endpoints: currently we are using Application Layer logic in the
    // REST API. This is not ideal and should be changed. The REST API should only be responsible for handling HTTP
    // Requests and Responses.
    // Furthermore, the Application Layer (not implemented yet) should be kept very thin as well. See: https://github.com/impierce/ssi-agent/issues/114

    // Accept the Credential Offer if it exists
    match query_handler(&offer_id, &state.query.received_offer).await {
        Ok(Some(ReceivedOfferView { .. })) => {
            let command = OfferCommand::AcceptCredentialOffer {
                offer_id: offer_id.clone(),
            };

            if command_handler(&offer_id, &state.command.offer, command).await.is_err() {
                // TODO: add better Error responses. This needs to be done properly in all endpoints once
                // https://github.com/impierce/openid4vc/issues/78 is fixed.
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }

    let command = OfferCommand::SendCredentialRequest {
        offer_id: offer_id.clone(),
    };

    // Send the Credential Request
    if command_handler(&offer_id, &state.command.offer, command).await.is_err() {
        // TODO: add better Error responses. This needs to be done properly in all endpoints once
        // https://github.com/impierce/openid4vc/issues/78 is fixed.
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let credentials = match query_handler(&offer_id, &state.query.received_offer).await {
        Ok(Some(ReceivedOfferView { credentials, .. })) => credentials,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    for credential in credentials {
        let credential_id = uuid::Uuid::new_v4().to_string();

        let command = CredentialCommand::AddCredential {
            credential_id: credential_id.clone(),
            offer_id: offer_id.clone(),
            credential,
        };

        // Add the Credential to the state.
        if command_handler(&credential_id, &state.command.credential, command)
            .await
            .is_err()
        {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    // TODO: What do we return here?
    StatusCode::OK.into_response()
}
