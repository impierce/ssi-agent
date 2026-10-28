use crate::error::IntoApiErrorExt;
use crate::extractors::RequestActor;
use crate::handlers::caller;
use agent_kms::key::aggregate::KeyReference;
use agent_kms::key_provider::aggregate::KeyProviderType;
use agent_kms::state::{create_key as create, get_keys};
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
#[schema(as = Key)]
pub struct KeyDto {
    pub id: String,
    pub provider_type: KeyProviderType,
    /// The provider's identifier for the key.
    pub provider_key_id: String,
    pub algorithm: JwsAlgorithm,
    pub public_key: Jwk,
}

impl KeyDto {
    fn new(id: String, reference: KeyReference) -> Self {
        let KeyReference {
            provider_type,
            provider_key_id,
            algorithm,
            public_key,
        } = reference;
        Self {
            id,
            provider_type,
            provider_key_id,
            algorithm,
            public_key,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateKeyRequest {
    pub provider_type: KeyProviderType,
    /// `ES256` or `EdDSA`.
    pub algorithm: JwsAlgorithm,
    /// A name for the key in the provider, which then identifies it there. Without one, the provider names it.
    pub label: Option<String>,
}

/// Create a key
///
/// Creates a key in an added key provider. The private key is generated in the provider and never leaves it.
/// Nothing signs with the key yet.
#[utoipa::path(
    post,
    path = "/create-key",
    operation_id = "create_key",
    tags = ["Key Management"],
    request_body = CreateKeyRequest,
    responses(
        (status = 201, description = "Key created", body = KeyDto),
        (status = 400, description = "The algorithm, or the label, is not supported by the provider"),
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
    let (id, reference) = create(&state, caller(actor), provider_type, algorithm, label)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    Ok((StatusCode::CREATED, Json(KeyDto::new(id, reference))).into_response())
}

/// List all keys
///
/// Lists the keys this instance created, in the order they were created.
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
