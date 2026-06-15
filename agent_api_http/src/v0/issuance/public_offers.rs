use crate::handlers::{command_handler, query_handler};
use agent_issuance::public_offer::aggregate::PublicOffer;
use agent_issuance::public_offer::command::PublicOfferCommand;
use agent_issuance::state::IssuanceState;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublicOfferStatusDto {
    pub id: String,
    pub template_id: String,
    pub amount_issued: u64,
    pub status: PublicOfferStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub enum PublicOfferStatus {
    Active,
    Inactive,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePublicOfferRequest {
    pub offer_id: String,
    pub template_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TakePublicOfferOfflineRequest {
    pub offer_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TakePublicOfferOnlineRequest {
    pub offer_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeletePublicOfferRequest {
    pub offer_id: String,
}

impl From<&PublicOffer> for PublicOfferStatusDto {
    fn from(offer: &PublicOffer) -> Self {
        PublicOfferStatusDto {
            id: offer.id.clone(),
            template_id: offer.template_id.clone(),
            amount_issued: 0,
            status: if offer.active && !offer.deleted {
                PublicOfferStatus::Active
            } else {
                PublicOfferStatus::Inactive
            },
        }
    }
}

/// Get all public offers
#[utoipa::path(
    get,
    path = "/get-all-public-offers",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Public offers retrieved successfully", body = [PublicOfferStatusDto])
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn all_public_offers(
    State(state): State<Arc<IssuanceState>>,
) -> Result<Response, ApiError> {
    let all_offers = query_handler("all_public_offers", &state.query.all_public_offers)
        .await?
        .unwrap_or_default();

    let mut offers = Vec::with_capacity(all_offers.offers.len());

    for public_offer in all_offers.offers.values() {
        let mut dto = PublicOfferStatusDto::from(public_offer);
        if let Some(offer_view) = query_handler(&public_offer.id, &state.query.offer).await? {
            dto.amount_issued = offer_view.successful_issuances;
        }
        offers.push(dto);
    }

    Ok((StatusCode::OK, Json(offers)).into_response())
}

/// Create a public offer mapping
#[utoipa::path(
    post,
    path = "/create-public-offer",
    tags = ["Issuance"],
    request_body = CreatePublicOfferRequest,
    responses(
        (status = 201, description = "Public offer created successfully"),
        (status = 404, description = "Template not found or offer already exists")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn create_public_offer(
    State(state): State<Arc<IssuanceState>>,
    Json(CreatePublicOfferRequest {
        offer_id,
        template_id,
    }): Json<CreatePublicOfferRequest>,
) -> Result<Response, ApiError> {
    if query_handler(&offer_id, &state.query.offer).await?.is_none() {
        return Err(ApiError::new(StatusCode::NOT_FOUND));
    }

    let command = PublicOfferCommand::Create {
        offer_id: offer_id.clone(),
        template_id,
    };

    command_handler(&offer_id, &state.command.public_offer, command).await?;

    Ok((StatusCode::CREATED).into_response())
}

/// Take a public offer offline
#[utoipa::path(
    post,
    path = "/take-public-offer-offline",
    tags = ["Issuance"],
    request_body = TakePublicOfferOfflineRequest,
    responses(
        (status = 204, description = "Public offer taken offline successfully"),
        (status = 404, description = "Public offer not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn take_public_offer_offline(
    State(state): State<Arc<IssuanceState>>,
    Json(TakePublicOfferOfflineRequest { offer_id }): Json<TakePublicOfferOfflineRequest>,
) -> Result<Response, ApiError> {
    let command = PublicOfferCommand::TakeOffline {
        offer_id: offer_id.clone(),
    };

    command_handler(&offer_id, &state.command.public_offer, command).await?;

    Ok((StatusCode::NO_CONTENT).into_response())
}

/// Take a public offer online
#[utoipa::path(
    post,
    path = "/take-public-offer-online",
    tags = ["Issuance"],
    request_body = TakePublicOfferOnlineRequest,
    responses(
        (status = 204, description = "Public offer taken online successfully"),
        (status = 404, description = "Public offer not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn take_public_offer_online(
    State(state): State<Arc<IssuanceState>>,
    Json(TakePublicOfferOnlineRequest { offer_id }): Json<TakePublicOfferOnlineRequest>,
) -> Result<Response, ApiError> {
    let command = PublicOfferCommand::TakeOnline {
        offer_id: offer_id.clone(),
    };

    command_handler(&offer_id, &state.command.public_offer, command).await?;

    Ok((StatusCode::NO_CONTENT).into_response())
}

/// Delete a public offer
#[utoipa::path(
    post,
    path = "/delete-public-offer",
    tags = ["Issuance"],
    request_body = DeletePublicOfferRequest,
    responses(
        (status = 204, description = "Public offer deleted successfully"),
        (status = 404, description = "Public offer not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn delete_public_offer(
    State(state): State<Arc<IssuanceState>>,
    Json(DeletePublicOfferRequest { offer_id }): Json<DeletePublicOfferRequest>,
) -> Result<Response, ApiError> {
    let command = PublicOfferCommand::Delete {
        offer_id: offer_id.clone(),
    };

    command_handler(&offer_id, &state.command.public_offer, command).await?;

    Ok((StatusCode::NO_CONTENT).into_response())
}

/// Check if a public offer can be resolved (is active and not deleted)
pub(crate) async fn can_resolve_public_offer(
    state: &Arc<IssuanceState>,
    offer_id: &str,
) -> Result<bool, ApiError> {
    match query_handler(offer_id, &state.query.public_offer).await? {
        Some(offer) => Ok(offer.active && !offer.deleted),
        // If there is no public-offer record, treat it as a normal offer.
        None => Ok(true),
    }
}

