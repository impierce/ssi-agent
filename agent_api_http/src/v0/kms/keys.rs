use crate::error::IntoApiErrorExt;
use crate::extractors::RequestActor;
use crate::handlers::caller;
use agent_kms::key::aggregate::KeyReference;
use agent_kms::key_provider::aggregate::KeyProviderType;
use agent_kms::state::{adopt_key as adopt, create_key as create, delete_key as delete, get_keys};
use agent_kms::KmsState;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http::StatusCode;
use http_api_problem::ApiError;
use identity_kit::jose::{Jwk, JwsAlgorithm};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A key this instance created in one of its key providers.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = Key)]
pub struct KeyDto {
    pub id: String,
    pub provider_type: KeyProviderType,
    /// The provider's identifier for the key, which names this key for good.
    pub provider_key_id: String,
    /// The label the key was created with, which also names it in the provider unless it is moved to another key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub algorithm: JwsAlgorithm,
    pub public_key: Jwk,
}

impl KeyDto {
    fn new(id: String, reference: KeyReference) -> Self {
        let KeyReference {
            provider_type,
            provider_key_id,
            label,
            algorithm,
            public_key,
        } = reference;
        Self {
            id,
            provider_type,
            provider_key_id,
            label,
            algorithm,
            public_key,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateKeyRequest {
    pub provider_type: KeyProviderType,
    pub algorithm: CreateKeyAlgorithm,
    /// A name for the key in the provider, which then identifies it there. Without one, the provider names it.
    ///
    /// A labelled request is safe to retry: when the first one created the key, the retry answers `409` instead of
    /// creating a second key.
    pub label: Option<String>,
}

/// The algorithms a key can be created for.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, utoipa::ToSchema)]
pub enum CreateKeyAlgorithm {
    ES256,
    EdDSA,
}

impl From<CreateKeyAlgorithm> for JwsAlgorithm {
    fn from(algorithm: CreateKeyAlgorithm) -> Self {
        match algorithm {
            CreateKeyAlgorithm::ES256 => JwsAlgorithm::ES256,
            CreateKeyAlgorithm::EdDSA => JwsAlgorithm::EdDSA,
        }
    }
}

/// Create a key
///
/// Creates a key in an added key provider. The private key is generated in the provider and never leaves it.
/// The key signs for the `did:web` document once it is added to it with `add-key-to-document` and selected with
/// `select-signing-key`.
#[utoipa::path(
    post,
    path = "/create-key",
    operation_id = "create_key",
    tags = ["Key Management"],
    request_body = CreateKeyRequest,
    responses(
        (status = 201, description = "Key created", body = KeyDto),
        (status = 400, description = "The provider does not support the algorithm or the label"),
        (status = 404, description = "No provider of this type has been added"),
        (status = 409, description = "The provider already has a key with this label"),
        (status = 422, description = "Request body does not match the expected schema"),
        (status = 502, description = "The provider could not create the key"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn create_key(
    State(state): State<Arc<KmsState>>,
    RequestActor(actor): RequestActor,
    Json(CreateKeyRequest {
        provider_type,
        algorithm,
        label,
    }): Json<CreateKeyRequest>,
) -> Result<Response, ApiError> {
    let (id, reference) = create(&state, caller(actor), provider_type, algorithm.into(), label)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    Ok((StatusCode::CREATED, Json(KeyDto::new(id, reference))).into_response())
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdoptKeyRequest {
    pub provider_type: KeyProviderType,
    /// The label that names the key in the provider.
    pub label: String,
}

/// Adopt a key
///
/// Records a key that exists in an added key provider under `label`, but that this instance didn't record, such as a
/// key whose creation was interrupted after the provider created it. `create-key` answers `409` for its label.
#[utoipa::path(
    post,
    path = "/adopt-key",
    operation_id = "adopt_key",
    tags = ["Key Management"],
    request_body = AdoptKeyRequest,
    responses(
        (status = 201, description = "Key adopted", body = KeyDto),
        (status = 400, description = "The key isn't of an algorithm keys can be created for, or the provider refuses it"),
        (status = 404, description = "No provider of this type has been added, or no key has this label"),
        (status = 409, description = "The key is already recorded"),
        (status = 422, description = "Request body does not match the expected schema"),
        (status = 502, description = "The provider could not be asked for the key"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn adopt_key(
    State(state): State<Arc<KmsState>>,
    RequestActor(actor): RequestActor,
    Json(AdoptKeyRequest { provider_type, label }): Json<AdoptKeyRequest>,
) -> Result<Response, ApiError> {
    let (id, reference) = adopt(&state, caller(actor), provider_type, label)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    Ok((StatusCode::CREATED, Json(KeyDto::new(id, reference))).into_response())
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteKeyRequest {
    pub id: String,
}

/// Delete a key
///
/// Deletes a key from its provider, which schedules its deletion where the provider does so, and frees its label. It
/// no longer signs. The provider doesn't need to be added anymore.
#[utoipa::path(
    post,
    path = "/delete-key",
    operation_id = "delete_key",
    tags = ["Key Management"],
    request_body = DeleteKeyRequest,
    responses(
        (status = 204, description = "Key deleted"),
        (status = 404, description = "There is no such key"),
        (status = 422, description = "Request body does not match the expected schema"),
        (status = 502, description = "The provider could not delete the key"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn delete_key(
    State(state): State<Arc<KmsState>>,
    RequestActor(actor): RequestActor,
    Json(DeleteKeyRequest { id }): Json<DeleteKeyRequest>,
) -> Result<Response, ApiError> {
    delete(&state, caller(actor), &id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// List all keys
///
/// Lists the keys this instance created or adopted and didn't delete, in the order they were recorded.
#[utoipa::path(
    get,
    path = "/get-all-keys",
    operation_id = "get_all_keys",
    tags = ["Key Management"],
    responses(
        (status = 200, description = "All keys retrieved successfully", body = [KeyDto]),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_all_keys(
    State(state): State<Arc<KmsState>>,
    RequestActor(actor): RequestActor,
) -> Result<Response, ApiError> {
    let keys: Vec<KeyDto> = get_keys(&state, caller(actor))
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .keys
        .into_iter()
        .map(|(id, reference)| KeyDto::new(id, reference))
        .collect();

    Ok((StatusCode::OK, Json(keys)).into_response())
}
