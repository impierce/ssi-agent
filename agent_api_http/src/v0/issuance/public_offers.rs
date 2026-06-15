use crate::handlers::query_handler;
use agent_issuance::state::IssuanceState;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{LazyLock, RwLock},
};

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

#[derive(Clone, Debug, Default)]
struct PublicOfferRecord {
    template_id: String,
    amount_issued: u64,
    active: bool,
    deleted: bool,
}

static PUBLIC_OFFERS: LazyLock<RwLock<HashMap<String, PublicOfferRecord>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub(crate) fn can_resolve_public_offer(offer_id: &str) -> bool {
    let public_offers = PUBLIC_OFFERS.read().expect("public offers lock poisoned");

    match public_offers.get(offer_id) {
        Some(record) => record.active && !record.deleted,
        None => true,
    }
}

pub(crate) fn increment_public_offer_claims(offer_id: &str) {
    let mut public_offers = PUBLIC_OFFERS.write().expect("public offers lock poisoned");

    if let Some(record) = public_offers.get_mut(offer_id) {
        if record.active && !record.deleted {
            record.amount_issued = record.amount_issued.saturating_add(1);
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
pub(crate) async fn all_public_offers() -> Result<Response, ApiError> {
    let public_offers = PUBLIC_OFFERS.read().expect("public offers lock poisoned");

    let payload = public_offers
        .iter()
        .filter(|(_, record)| !record.deleted)
        .map(|(offer_id, record)| PublicOfferStatusDto {
            id: offer_id.clone(),
            template_id: record.template_id.clone(),
            amount_issued: record.amount_issued,
            status: if record.active {
                PublicOfferStatus::Active
            } else {
                PublicOfferStatus::Inactive
            },
        })
        .collect::<Vec<_>>();

    Ok((StatusCode::OK, Json(payload)).into_response())
}

/// Create a public offer mapping
#[utoipa::path(
    post,
    path = "/create-public-offer",
    tags = ["Issuance"],
    request_body = CreatePublicOfferRequest,
    responses(
        (status = 201, description = "Public offer created successfully"),
        (status = 404, description = "Offer not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn create_public_offer(
    State(state): State<std::sync::Arc<IssuanceState>>,
    Json(CreatePublicOfferRequest { offer_id, template_id }): Json<CreatePublicOfferRequest>,
) -> Result<Response, ApiError> {
    if query_handler(&offer_id, &state.query.offer).await?.is_none() {
        return Err(ApiError::new(StatusCode::NOT_FOUND));
    }

    let mut public_offers = PUBLIC_OFFERS.write().expect("public offers lock poisoned");

    public_offers
        .entry(offer_id)
        .and_modify(|record: &mut PublicOfferRecord| {
            record.template_id = template_id.clone();
            record.active = true;
            record.deleted = false;
        })
        .or_insert(PublicOfferRecord {
            template_id,
            amount_issued: 0,
            active: true,
            deleted: false,
        });

    Ok(StatusCode::CREATED.into_response())
}

/// Take a public offer offline
#[utoipa::path(
    post,
    path = "/take-public-offer-offline/{offer_id}",
    tags = ["Issuance"],
    responses(
        (status = 204, description = "Public offer is now inactive"),
        (status = 404, description = "Public offer not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn take_public_offer_offline(Path(offer_id): Path<String>) -> Result<Response, ApiError> {
    let mut public_offers = PUBLIC_OFFERS.write().expect("public offers lock poisoned");

    let Some(record) = public_offers.get_mut(&offer_id) else {
        return Err(ApiError::new(StatusCode::NOT_FOUND));
    };

    if record.deleted {
        return Err(ApiError::new(StatusCode::NOT_FOUND));
    }

    record.active = false;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Take a public offer online
#[utoipa::path(
    post,
    path = "/take-public-offer-online/{offer_id}",
    tags = ["Issuance"],
    responses(
        (status = 204, description = "Public offer is now active"),
        (status = 404, description = "Public offer not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn take_public_offer_online(Path(offer_id): Path<String>) -> Result<Response, ApiError> {
    let mut public_offers = PUBLIC_OFFERS.write().expect("public offers lock poisoned");

    let Some(record) = public_offers.get_mut(&offer_id) else {
        return Err(ApiError::new(StatusCode::NOT_FOUND));
    };

    if record.deleted {
        return Err(ApiError::new(StatusCode::NOT_FOUND));
    }

    record.active = true;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Delete a public offer
#[utoipa::path(
    delete,
    path = "/remove-public-offer/{offer_id}",
    tags = ["Issuance"],
    responses(
        (status = 204, description = "Public offer deleted"),
        (status = 404, description = "Public offer not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn delete_public_offer(Path(offer_id): Path<String>) -> Result<Response, ApiError> {
    let mut public_offers = PUBLIC_OFFERS.write().expect("public offers lock poisoned");

    let Some(record) = public_offers.get_mut(&offer_id) else {
        return Err(ApiError::new(StatusCode::NOT_FOUND));
    };

    record.active = false;
    record.deleted = true;

    Ok(StatusCode::NO_CONTENT.into_response())
}
