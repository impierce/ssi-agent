use crate::handlers::{command_handler, query_handler};
use agent_holder::{
    credential::{aggregate::Credential, command::CredentialCommand},
    state::HolderState,
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use identity_credential::credential::Jwt;
use serde::{Deserialize, Serialize};
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
pub(crate) async fn credentials(State(state): State<Arc<HolderState>>) -> Result<Response, ApiError> {
    let all_credentials = query_handler("all_holder_credentials", &state.query.all_holder_credentials)
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
    Json(HolderCredentialsEndpointRequest { credential }): Json<HolderCredentialsEndpointRequest>,
) -> Result<Response, ApiError> {
    let holder_credential_id = uuid::Uuid::new_v4().to_string();

    let command = CredentialCommand::AddCredential {
        holder_credential_id: holder_credential_id.clone(),
        received_offer_id: None,
        credential,
    };

    command_handler(&state, &holder_credential_id, &state.command.credential, command).await?;

    query_handler(&holder_credential_id, &state.query.holder_credential)
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
    Path(holder_credential_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&holder_credential_id, &state.query.holder_credential)
        .await?
        .map(|holder_credential_view| (StatusCode::OK, Json(holder_credential_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_holder::services::HolderServices;
    use agent_issuance::credential::aggregate::test_utils::JWT_VC_JSON_OBV3_JWT;
    use agent_secret_manager::service::Service;
    use agent_store::{holder_state, in_memory::InMemory};

    #[tokio::test]
    async fn post_credentials_dispatches_add_credential_command() {
        let state = Arc::new(holder_state(&InMemory, HolderServices::default().await, Default::default()).await);

        let response = post_credentials(
            State(state),
            Json(HolderCredentialsEndpointRequest {
                credential: Jwt::from(JWT_VC_JSON_OBV3_JWT.to_string()),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
    }
}
