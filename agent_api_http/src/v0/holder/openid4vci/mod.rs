use crate::handlers::public_command_handler;
use crate::v0::openapi::PROTOCOL_TAG;
use agent_holder::{offer::command::OfferCommand, state::HolderState};
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use oid4vci::credential_offer::CredentialOffer;
use serde::Deserialize;
use std::sync::Arc;
use tracing::info;

/// A credential offer passed by value or by reference. Exactly one of the two parameters is expected.
#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct CredentialOfferQuery {
    /// JSON-encoded credential offer.
    credential_offer: Option<String>,
    /// URL from which the credential offer can be retrieved.
    credential_offer_uri: Option<String>,
}

/// Receive a credential offer
///
/// Receives a credential offer from a credential issuer, as defined by
/// [OpenID4VCI](https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-credential-offer-endpoint).
/// `credential_offer` takes precedence when both parameters are present.
#[utoipa::path(
    get,
    path = "/credential_offer",
    operation_id = "credential_offer",
    tags = ["OpenID4VCI", PROTOCOL_TAG],
    params(CredentialOfferQuery),
    responses(
        (status = 200, description = "Credential offer received"),
        (status = 400, description = "Neither parameter is present, or the credential offer is invalid"),
        (status = 500, description = "The credential offer could not be stored"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn offers_params(
    State(state): State<Arc<HolderState>>,
    // TODO: Can this be changed to `StringifiedForm`?
    Query(payload): Query<CredentialOfferQuery>,
) -> Result<Response, ApiError> {
    let credential_offer_result: Result<CredentialOffer, _> =
        if let Some(credential_offer) = payload.credential_offer {
            format!("openid-credential-offer://?credential_offer={credential_offer}")
        } else if let Some(credential_offer_uri) = payload.credential_offer_uri {
            format!("openid-credential-offer://?credential_offer_uri={credential_offer_uri}")
        } else {
            return Err(ApiError::new(StatusCode::BAD_REQUEST));
        }
        .parse();

    let credential_offer = match credential_offer_result {
        Ok(credential_offer) => credential_offer,
        Err(_) => return Err(ApiError::new(StatusCode::BAD_REQUEST)),
    };

    let received_offer_id = uuid::Uuid::new_v4().to_string();

    info!("Credential Offer: {:#?}", credential_offer);

    let command = OfferCommand::ReceiveCredentialOffer {
        received_offer_id: received_offer_id.clone(),
        credential_offer,
    };

    // Add the Credential Offer to the state.
    public_command_handler(&received_offer_id, &state.command.offer, command).await?;

    Ok(StatusCode::OK.into_response())
}
