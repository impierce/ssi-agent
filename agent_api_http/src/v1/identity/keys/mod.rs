use crate::IdentityContext;
use agent_identity::managed_key::aggregate::{ManagedKey, SigningAlgorithm};
use agent_identity::managed_key::command::ManagedKeyCommand;
use agent_identity::managed_key::views::all_managed_keys::AllManagedKeysView;

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

// impl From<AllManagedKeysView> for ManagedKeyDto {
//     fn from(view: AllManagedKeysView) -> Self {
//         Self {
//             managed_key_id: view.managed_key_id,
//             key_id: view.key_id,
//             alias: view.alias,
//             // Check if the view stores the algorithm as a String or Enum
//             // You might need .map(|a| a.to_string()) if it's an Option<Enum>
//             signing_algorithm: view.signing_algorithm.map(|alg| alg.to_string()),
//         }
//     }
// }
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
    let new_managed_key_id = uuid::Uuid::new_v4().to_string();

    let command = ManagedKeyCommand::GenerateKey {
        managed_key_id: new_managed_key_id.clone(),
        alias: payload.alias,
        signing_algorithm: payload.signature_algorithm.unwrap_or_else(|| SigningAlgorithm::ES256),
    };

    context.key_generation_saga.execute(command).await.map_err(|e| {
        tracing::error!("Saga failed: {:?}", e);
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
    })?;
    Ok((StatusCode::CREATED, new_managed_key_id))
}

#[derive(Deserialize)]
pub struct PostRemoveKey {
    pub key_id: String,
}

pub(crate) async fn remove_key(
    State(context): State<IdentityContext>,
    Json(payload): Json<PostRemoveKey>,
) -> Result<StatusCode, ApiError> {
    let managed_key_id = query_handler(&payload.key_id, &context.state.query.managed_key)
        .await?
        .map(|key_view| key_view.managed_key_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))?;

    context.key_removal_saga.execute(managed_key_id).await.map_err(|e| {
        tracing::error!("Saga failed: {:?}", e);
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
    })?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
pub struct PostRenameAlias {
    pub key_id: String,
    pub new_alias: String,
}

pub(crate) async fn rename_key_alias(
    State(context): State<IdentityContext>,
    Json(payload): Json<PostRenameAlias>,
) -> Result<StatusCode, ApiError> {
    let managed_key_id = query_handler(&payload.key_id, &context.state.query.managed_key)
        .await?
        .map(|key_view| key_view.managed_key_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))?;

    let command = ManagedKeyCommand::UpdateKeyAlias {
        new_alias: payload.new_alias,
    };

    command_handler(&managed_key_id, &context.state.command.managed_key, command).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PostSetSigningKey {
    pub key_id: String,
}

pub(crate) async fn set_signing_key(
    State(context): State<IdentityContext>,
    Json(payload): Json<PostSetSigningKey>,
) -> Result<StatusCode, ApiError> {
    let managed_key_id = query_handler(&payload.key_id, &context.state.query.managed_key)
        .await?
        .map(|key_view| key_view.managed_key_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))?;

    let command = ManagedKeyCommand::SetSigningKey {};

    command_handler(&managed_key_id, &context.state.command.managed_key, command).await?;
    Ok(StatusCode::NO_CONTENT)
}

// #[axum_macros::debug_handler]
// pub(crate) async fn list_all(
//     State(context): State<IdentityContext>,
// ) -> Result<(StatusCode, Json<Vec<ManagedKeyDto>>), ApiError> {
//     let view = query_handler(&(), &context.state.query.all_managed_keys).await?;

//     // AllManagedKeysView contains a HashMap of ManagedKeyViews
//     let keys = view
//         .managed_keys
//         .into_values() // Iterate over the 'ManagedKeyView' items inside
//         .map(|key_view| ManagedKeyDto {
//             // Map the fields manually (safest way)
//             // Assuming ManagedKeyView has similar fields to ManagedKey
//             managed_key_id: key_view.managed_key_id,
//             key_id: key_view.key_id,
//             alias: key_view.alias,
//             // Handle the enum-to-string conversion
//             signing_algorithm: key_view.signing_algorithm.map(|alg| alg.to_string()),
//         })
//         .collect();

//     Ok((StatusCode::OK, Json(keys)))
// }
