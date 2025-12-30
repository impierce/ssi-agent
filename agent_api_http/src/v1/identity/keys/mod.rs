use crate::IdentityContext;
use agent_identity::managed_key::aggregate::{ManagedKey, SigningAlgorithm};
use agent_identity::managed_key::command::ManagedKeyCommand;

use agent_shared::handlers::{command_handler, query_handler};
use axum::extract::{Json, State};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedKeyDto {
    #[serde(rename = "id")]
    pub managed_key_id: String,
    pub key_id: String,
    pub alias: String,
    pub signing_algorithm: Option<SigningAlgorithm>,
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
    pub signature_algorithm: Option<SigningAlgorithm>,
}

#[axum_macros::debug_handler]
pub(crate) async fn generate_key(
    State(context): State<IdentityContext>,
    Json(payload): Json<PostGenerateKey>,
) -> Result<(StatusCode, String), ApiError> {
    let signing_algorithm = payload.signature_algorithm.unwrap_or(SigningAlgorithm::ES256);

    let managed_key_id = context
        .key_generation_saga
        .generate_key(payload.alias, signing_algorithm)
        .await
        .map_err(|e| {
            tracing::error!("Saga failed: {:?}", e);
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    Ok((StatusCode::CREATED, managed_key_id))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostRemoveKey {
    pub key_id: String,
}

pub(crate) async fn remove_key(
    State(context): State<IdentityContext>,
    Json(payload): Json<PostRemoveKey>,
) -> Result<StatusCode, ApiError> {
    let managed_key_id = get_managed_key_id(&payload.key_id, &context).await?;

    context.key_removal_saga.remove_key(managed_key_id).await;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostRenameAlias {
    pub key_id: String,
    pub new_alias: String,
}

pub(crate) async fn rename_key_alias(
    State(context): State<IdentityContext>,
    Json(payload): Json<PostRenameAlias>,
) -> Result<StatusCode, ApiError> {
    let managed_key_id = get_managed_key_id(&payload.key_id, &context).await?;

    let command = ManagedKeyCommand::UpdateKeyAlias {
        new_alias: payload.new_alias,
    };

    command_handler(&managed_key_id, &context.state.command.managed_key, command)
        .await
        .map_err(|e| {
            tracing::error!("Saga failed: {:?}", e);
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostSetSigningKey {
    pub key_id: String,
}

pub(crate) async fn set_signing_key(
    State(context): State<IdentityContext>,
    Json(payload): Json<PostSetSigningKey>,
) -> Result<StatusCode, ApiError> {
    let managed_key_id = get_managed_key_id(&payload.key_id, &context).await?;

    let command = ManagedKeyCommand::SetSigningKey {};

    command_handler(&managed_key_id, &context.state.command.managed_key, command)
        .await
        .map_err(|e| {
            tracing::error!("Saga failed: {:?}", e);
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    Ok(StatusCode::NO_CONTENT)
}

#[axum_macros::debug_handler]
pub(crate) async fn list_all(
    State(context): State<IdentityContext>,
) -> Result<(StatusCode, Json<Vec<ManagedKeyDto>>), ApiError> {
    let view = query_handler("all_managed_keys", &context.state.query.all_managed_keys)
        .await
        .map_err(|e| {
            tracing::error!("Query failed: {:?}", e);
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    let keys = match view {
        Some(all_keys_view) => all_keys_view
            .managed_keys
            .into_values()
            .filter_map(|key_view| {
                (!key_view.is_removed).then(|| ManagedKeyDto {
                    managed_key_id: key_view.managed_key_id,
                    key_id: key_view.key_id,
                    alias: key_view.alias,
                    signing_algorithm: key_view.signing_algorithm,
                })
            })
            .collect(),
        None => Vec::new(),
    };

    Ok((StatusCode::OK, Json(keys)))
}

// Helper function to find the managed_key_id by key_id
async fn get_managed_key_id(key_id: &str, context: &IdentityContext) -> Result<String, ApiError> {
    let view = query_handler("all_managed_keys", &context.state.query.all_managed_keys)
        .await
        .map_err(|e| {
            tracing::error!("Query failed: {:?}", e);
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    match view {
        Some(all_keys_view) => all_keys_view
            .managed_keys
            .into_values()
            .find(|key_view| key_view.key_id == key_id && !key_view.is_removed)
            .map(|key_view| key_view.managed_key_id)
            .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND)),
        None => Err(ApiError::new(StatusCode::NOT_FOUND)),
    }
}
