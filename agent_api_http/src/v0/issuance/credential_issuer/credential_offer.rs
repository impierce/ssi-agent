use crate::handlers::public_query_handler;
use crate::v0::issuance::public_offers::can_resolve_public_offer;
use crate::v0::openapi::PROTOCOL_TAG;
use agent_issuance::{offer::aggregate::Offer, state::IssuanceState};
use axum::{
    extract::{Path, State},
    response::{IntoResponse as _, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use oid4vci::credential_offer::CredentialOffer;
use oid4vci::credential_offer::CredentialOfferParameters;
use std::sync::Arc;

/// Get a credential offer
///
/// Returns the credential offer referenced by a `credential_offer_uri`, as defined by
/// [OpenID4VCI](https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-sending-credential-offer-by-).
#[utoipa::path(
    get,
    path = "/openid4vci/credential-offer/{offer_id}",
    operation_id = "openid4vci_credential_offer",
    tags = ["OpenID4VCI", PROTOCOL_TAG],
    params(
        ("offer_id" = String, Path, description = "Credential offer ID"),
    ),
    responses(
        (status = 200, description = "Credential offer", body = CredentialOfferParameters),
        (status = 404, description = "The credential offer does not exist or cannot be resolved"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential_offer_uri(
    State(state): State<Arc<IssuanceState>>,
    Path(offer_id): Path<String>,
) -> Result<Response, ApiError> {
    if !can_resolve_public_offer(&state, &offer_id).await? {
        return Err(ApiError::new(StatusCode::NOT_FOUND));
    }

    match public_query_handler(&offer_id, &state.query.offer).await? {
        Some(Offer {
            credential_offer: Some(CredentialOffer::CredentialOffer(credential_offer_parameters)),
            ..
        }) => Ok((StatusCode::OK, Json(credential_offer_parameters)).into_response()),
        _ => Err(ApiError::new(StatusCode::NOT_FOUND)),
    }
}
