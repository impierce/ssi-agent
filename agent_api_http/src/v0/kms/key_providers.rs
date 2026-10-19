use crate::error::IntoApiErrorExt;
use crate::extractors::RequestActor;
use crate::handlers::{caller, command_handler, query_handler};
use agent_kms::key_provider::{
    aggregate::KeyProviderType,
    command::KeyProviderCommand,
    views::{all_key_providers::AllKeyProvidersView, KeyProviderView},
};
use agent_kms::state::{get_key_provider, test_key_provider_connection as test_connection};
use agent_kms::KmsState;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http::StatusCode;
use http_api_problem::ApiError;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Caller;
use std::sync::Arc;

/// A key provider this instance is connected to.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = KeyProvider)]
pub struct KeyProviderDto {
    pub provider_type: KeyProviderType,
    pub name: String,
}

impl KeyProviderDto {
    fn from_view(view: KeyProviderView) -> Option<Self> {
        Some(Self {
            provider_type: view.provider_type?,
            name: view.name,
        })
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddKeyProviderRequest {
    pub provider_type: KeyProviderType,
    /// A name for the provider, unique across all providers. It can't be changed later.
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeyProviderTypeRequest {
    pub provider_type: KeyProviderType,
}

/// The outcome of a connection test. It is not stored.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
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
    command_handler(
        state.authorization_checker.clone(),
        actor,
        &provider_type.to_string(),
        &state.command.key_provider,
        KeyProviderCommand::AddKeyProvider { provider_type, name },
    )
    .await?;

    get_key_provider(&state, Caller::Internal, provider_type)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .and_then(KeyProviderDto::from_view)
        .map(|provider| (StatusCode::CREATED, Json(provider)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
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
    let providers: Vec<KeyProviderDto> = query_handler(
        state.authorization_checker.clone(),
        actor,
        AllKeyProvidersView::VIEW_ID,
        None,
        &state.query.all_key_providers,
    )
    .await?
    .map(|view| {
        view.key_providers
            .into_values()
            .filter_map(KeyProviderDto::from_view)
            .collect()
    })
    .unwrap_or_default();

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
        &provider_type.to_string(),
        &state.command.key_provider,
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
