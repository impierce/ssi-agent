use crate::error::{type_url, IntoApiErrorExt};
use crate::extractors::RequestActor;
use crate::handlers::{caller, command_handler};
use agent_kms::adapter::ProviderKeys;
use agent_kms::key_provider::{
    aggregate::{KeyProviderType, KeyProviders},
    command::KeyProviderCommand,
};
use agent_kms::state::{get_key_providers, list_key_provider_keys, test_key_provider_connection as test_connection};
use agent_kms::KmsState;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http::StatusCode;
use http_api_problem::ApiError;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A key provider this instance is connected to.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = KeyProvider)]
pub struct KeyProviderDto {
    pub provider_type: KeyProviderType,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddKeyProviderRequest {
    pub provider_type: KeyProviderType,
    /// A name for the provider, unique among the providers this instance lists. It can't be changed later.
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeyProviderTypeRequest {
    pub provider_type: KeyProviderType,
}

/// The outcome of a connection test. It is not stored.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub reachable: bool,
    /// Why the provider could not be reached, as reported by the provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Add a key provider
///
/// Connects this instance to a key provider. Only one provider of each type can be added. The in-memory provider
/// is only available in the development profile.
#[utoipa::path(
    post,
    path = "/add-key-provider",
    operation_id = "add_key_provider",
    tags = ["Key Management"],
    request_body = AddKeyProviderRequest,
    responses(
        (status = 201, description = "Key provider added", body = KeyProviderDto),
        (status = 400, description = "Missing name, or a provider type that is not available in this profile"),
        (status = 409, description = "A provider of this type, or with this name, already exists"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn add_key_provider(
    State(state): State<Arc<KmsState>>,
    RequestActor(actor): RequestActor,
    Json(AddKeyProviderRequest { provider_type, name }): Json<AddKeyProviderRequest>,
) -> Result<Response, ApiError> {
    let name = KeyProviders::normalize_name(&name);
    command_handler(
        state.authorization_checker.clone(),
        actor,
        KeyProviders::ID,
        &state.command.key_providers,
        KeyProviderCommand::AddKeyProvider {
            provider_type,
            name: name.clone(),
        },
    )
    .await?;

    // Answer from the accepted command rather than the view: the event is stored at this point, but a failed view
    // update would otherwise turn a successful add into an error.
    Ok((StatusCode::CREATED, Json(KeyProviderDto { provider_type, name })).into_response())
}

/// List all key providers
///
/// Lists the key providers this instance is connected to, in the order they were added.
#[utoipa::path(
    get,
    path = "/get-all-key-providers",
    operation_id = "get_all_key_providers",
    tags = ["Key Management"],
    responses(
        (status = 200, description = "All key providers retrieved successfully", body = [KeyProviderDto]),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_all_key_providers(
    State(state): State<Arc<KmsState>>,
    RequestActor(actor): RequestActor,
) -> Result<Response, ApiError> {
    let providers: Vec<KeyProviderDto> = get_key_providers(&state, caller(actor))
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .providers
        .into_iter()
        .map(|(provider_type, name)| KeyProviderDto { provider_type, name })
        .collect();

    Ok((StatusCode::OK, Json(providers)).into_response())
}

/// Remove a key provider
///
/// Disconnects this instance from a key provider. No keys are deleted at the provider.
#[utoipa::path(
    post,
    path = "/remove-key-provider",
    operation_id = "remove_key_provider",
    tags = ["Key Management"],
    request_body = KeyProviderTypeRequest,
    responses(
        (status = 204, description = "Key provider removed"),
        (status = 404, description = "No provider of this type has been added"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn remove_key_provider(
    State(state): State<Arc<KmsState>>,
    RequestActor(actor): RequestActor,
    Json(KeyProviderTypeRequest { provider_type }): Json<KeyProviderTypeRequest>,
) -> Result<Response, ApiError> {
    command_handler(
        state.authorization_checker.clone(),
        actor,
        KeyProviders::ID,
        &state.command.key_providers,
        KeyProviderCommand::RemoveKeyProvider { provider_type },
    )
    .await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Test a key provider connection
///
/// Checks whether an added key provider can be reached with the current credentials. The outcome is returned but
/// not stored, so an unreachable provider is a successful response with `reachable` set to `false`.
#[utoipa::path(
    post,
    path = "/test-key-provider-connection",
    operation_id = "test_key_provider_connection",
    tags = ["Key Management"],
    request_body = KeyProviderTypeRequest,
    responses(
        (status = 200, description = "The connection was tested", body = ConnectionTestResult),
        (status = 404, description = "No provider of this type has been added"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn test_key_provider_connection(
    State(state): State<Arc<KmsState>>,
    RequestActor(actor): RequestActor,
    Json(KeyProviderTypeRequest { provider_type }): Json<KeyProviderTypeRequest>,
) -> Result<Response, ApiError> {
    let result = match test_connection(&state, caller(actor), provider_type)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
    {
        Ok(()) => ConnectionTestResult {
            reachable: true,
            message: None,
        },
        Err(error) => ConnectionTestResult {
            reachable: false,
            message: Some(error.to_string()),
        },
    };

    Ok((StatusCode::OK, Json(result)).into_response())
}

/// List the keys in a key provider
///
/// Lists the keys that exist in an added key provider and that this instance's credentials can see, as reported
/// by the provider. Nothing is stored, and the keys are not used by UniCore yet.
#[utoipa::path(
    get,
    path = "/get-key-provider-keys/{provider_type}",
    operation_id = "get_key_provider_keys",
    tags = ["Key Management"],
    params(("provider_type" = KeyProviderType, Path, description = "The type of the added key provider")),
    responses(
        (status = 200, description = "The keys this instance may use, and how many more it may not", body = ProviderKeys),
        (status = 404, description = "No provider of this type has been added"),
        (status = 502, description = "The provider could not be reached"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_key_provider_keys(
    State(state): State<Arc<KmsState>>,
    RequestActor(actor): RequestActor,
    Path(provider_type): Path<KeyProviderType>,
) -> Result<Response, ApiError> {
    let keys = list_key_provider_keys(&state, caller(actor), provider_type)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .map_err(|error| {
            ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Key Provider Unreachable")
                .type_url(type_url("kms#key-provider-unreachable"))
                .message(error.to_string())
                .finish()
        })?;

    Ok((StatusCode::OK, Json(keys)).into_response())
}
