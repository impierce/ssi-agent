use agent_identity::state::IdentityState;
use agent_shared::handlers::{command_handler, query_handler};
use axum::extract::{Json, State};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedKeyDto {
    #[serde(rename = "id")]
    pub managed_key_id: String,
    pub key_id: String,
    pub alias: Option<String>,
    pub signing_algorithm: Option<String>,
}

impl From<ManagedKey> for ManagedKeyDto {
    fn from(value: ManagedKey) -> Self {
        Self {
            managed_key_id: value.managed_key_id,
            key_id: value.key_id,
            alias: value.alias,
            signing_algorithm: value.signing_algorithm,
        }
    }
}

#[derive(Deserialize, Serialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct PostGenerateKey {
    pub alias: String,
    pub signature_algorithm: Option<String>,
    // Update with the new algorithm enum.
}

#[axum_macros::debug_handler]
pub(crate) async fn generate_key(
    State(state): State<Arc<IdentityState>>,
    Json(payload): Json<PostGenerateKey>,
) -> Result<(StatusCode, String), ApiError> {
    let managed_key_id = state
        .generate_key(&payload.alias, payload.signature_algorithm)
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))?;

    Ok((StatusCode::CREATED, managed_key_id))
}

#[derive(Deserialize)]
pub struct PostRemoveKey {
    pub key_id: String,
}

pub(crate) async fn remove_key(
    State(state): State<Arc<IdentityState>>,
    Json(payload): Json<PostRemoveKey>,
) -> Result<StatusCode, ApiError> {
    let managed_key_id = query_handler(&payload.key_id, &state.query.managed_key_id)
        .await?
        .map(|key_view| key_view.managed_key_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))?;

    let command = KeyCommand::RemoveKey {};

    command_handler(&managed_key_id, &state.command.keys, command).await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
pub struct PostRenameAlias {
    pub key_id: String,
    pub new_alias: String,
}

pub(crate) async fn rename_key_alias(
    State(state): State<Arc<IdentityState>>,
    Json(payload): Json<PostRenameAlias>,
) -> Result<StatusCode, ApiError> {
    let managed_key_id = query_handler(&payload.key_id, &state.query.managed_key_id)
        .await?
        .map(|key_view| key_view.managed_key_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))?;

    let command = KeyCommand::RenameAlias {
        new_alias: payload.new_alias,
    };

    command_handler(&managed_key_id, &state.command.keys, command).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PostSetSigningKey {
    pub key_id: String,
}

pub(crate) async fn set_signing_key(
    State(state): State<Arc<IdentityState>>,
    Json(payload): Json<PostSetSigningKey>,
) -> Result<StatusCode, ApiError> {
    let managed_key_id = query_handler(&payload.key_id, &state.query.managed_key_id)
        .await?
        .map(|key_view| key_view.managed_key_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))?;

    let command = KeyCommand::SetSigningKey {};

    command_handler(&managed_key_id, &state.command.keys, command).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[axum_macros::debug_handler]
pub(crate) async fn list_all(
    State(secret_manager): State<Arc<IdentityState>>,
) -> Result<(StatusCode, Json<Vec<ManagedKeyDto>>), ApiError> {
    let keys = query_handler(&(), &secret_manager.query.managed_keys)
        .await?
        .into_iter()
        .map(ManagedKeyDto::from)
        .collect();

    Ok((StatusCode::OK, Json(keys)))
}
