use agent_holder::{offer::command::OfferCommand, state::HolderState};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Form, Json,
};
use hyper::StatusCode;
use oid4vci::credential_offer::CredentialOffer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

    let received_offer_id = uuid::Uuid::new_v4().to_string();

    let command = OfferCommand::ReceiveCredentialOffer {
        received_offer_id: received_offer_id.clone(),
        credential_offer,
    };

    // Add the Credential Offer to the state.
    match command_handler(&received_offer_id, &state.command.offer, command).await {
        Ok(_) => StatusCode::OK.into_response(),
        // TODO: add better Error responses. This needs to be done properly in all endpoints once
        // https://github.com/impierce/openid4vc/issues/78 is fixed.
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[axum_macros::debug_handler]
pub(crate) async fn offers_params(
    State(state): State<HolderState>,
    Form(payload): Form<serde_json::Value>,
) -> Response {
    offers_inner(state, payload).await
}

pub(crate) async fn offers_inner(state: HolderState, payload: serde_json::Value) -> Response {
    info!("Request Body: {}", payload);

    let credential_offer_result: Result<CredentialOffer, _> =
        if let Some(credential_offer) = payload.get("credential_offer").and_then(Value::as_str) {
            format!("openid-credential-offer://?credential_offer={credential_offer}")
        } else if let Some(credential_offer_uri) = payload.get("credential_offer_uri").and_then(Value::as_str) {
            format!("openid-credential-offer://?credential_offer_uri={credential_offer_uri}")
        } else {
            return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
        }
        .parse();

    let credential_offer = match credential_offer_result {
        Ok(credential_offer) => credential_offer,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid payload").into_response(),
    };

    let received_offer_id = uuid::Uuid::new_v4().to_string();

    info!("Credential Offer: {:#?}", credential_offer);

    let command = OfferCommand::ReceiveCredentialOffer {
        received_offer_id: received_offer_id.clone(),
        credential_offer,
    };

    // Add the Credential Offer to the state.
    if command_handler(&received_offer_id, &state.command.offer, command)
        .await
        .is_err()
    {
        // TODO: add better Error responses. This needs to be done properly in all endpoints once
        // https://github.com/impierce/openid4vc/issues/78 is fixed.
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    match query_handler(&received_offer_id, &state.query.received_offer).await {
        // TODO: add Location header
        Ok(Some(received_offer)) => (StatusCode::CREATED, Json(received_offer)).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
