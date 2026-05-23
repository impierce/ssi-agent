use crate::handlers::{command_handler, load_view, query_handler, request_actor};
use agent_holder::{
    credential::{aggregate::Credential, command::CredentialCommand},
    state::HolderState,
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Extension, Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use identity_credential::credential::Jwt;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Actor;
use std::sync::Arc;

/// List all credentials
///
/// Retrieves all credentials held by your organisation.
#[utoipa::path(
    get,
    path = "/holder/credentials",
    operation_id = "get_all_holder_credentials",
    tags = ["Identity", "Holder"],
    responses(
        (status = 200, description = "All credentials retrieved successfully", body = [Credential]),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credentials(
    State(state): State<Arc<HolderState>>,
    actor: Option<Extension<Option<Actor>>>,
) -> Result<Response, ApiError> {
    let all_credentials = query_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        "all_holder_credentials",
        &state.query.all_holder_credentials,
    )
    .await?
    .map(|all_credentials_view| all_credentials_view.credentials.into_values().collect::<Vec<_>>())
    .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_credentials)).into_response())
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HolderCredentialsEndpointRequest {
    pub credential: Jwt,
}

#[axum_macros::debug_handler]
pub(crate) async fn post_credentials(
    State(state): State<Arc<HolderState>>,
    actor: Option<Extension<Option<Actor>>>,
    Json(HolderCredentialsEndpointRequest { credential }): Json<HolderCredentialsEndpointRequest>,
) -> Result<Response, ApiError> {
    let holder_credential_id = uuid::Uuid::new_v4().to_string();

    let command = CredentialCommand::AddCredential {
        holder_credential_id: holder_credential_id.clone(),
        received_offer_id: None,
        credential,
    };

    command_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &holder_credential_id,
        &state.command.credential,
        command,
    )
    .await?;

    load_view(&holder_credential_id, &state.query.holder_credential)
        .await?
        .map(|holder_credential_view| (StatusCode::CREATED, Json(holder_credential_view)).into_response())
        // TODO: this *should* be an impossible error, what should we return here?
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

/// Get credential by ID
///
/// Retrieves a credential held by your organisation by its ID.
#[utoipa::path(
    get,
    path = "/holder/credentials/{holder_credential_id}",
    operation_id = "get_holder_credential_by_id",
    tags = ["Identity", "Holder"],
    responses(
        (status = 200, description = "Credential retrieved successfully", body = Credential),
        (status = 404, description = "Credential not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential(
    State(state): State<Arc<HolderState>>,
    actor: Option<Extension<Option<Actor>>>,
    Path(holder_credential_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &holder_credential_id,
        &state.query.holder_credential,
    )
    .await?
    .map(|holder_credential_view| (StatusCode::OK, Json(holder_credential_view)).into_response())
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
