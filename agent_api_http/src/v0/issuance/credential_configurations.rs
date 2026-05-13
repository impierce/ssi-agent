use crate::handlers::{command_handler, request_actor};
use agent_issuance::server_config::command::ServerConfigCommand;
use agent_issuance::state::{IssuanceState, SERVER_CONFIG_ID};
use agent_shared::config::CredentialConfiguration;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension,
};
use http_api_problem::ApiError;
use shared_kernel::authorization::Actor;
use std::sync::Arc;

/// Update credential configuration
///
/// Publishes the provided credential configuration.
#[utoipa::path(
    post,
    path = "/credential-configurations",
    operation_id = "set_credential_configuration",
    tags = ["Issuance"],
    responses(
        (status = 201, description = "Credential configuration updated successfully")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential_configurations(
    State(state): State<Arc<IssuanceState>>,
    actor: Option<Extension<Option<Actor>>>,
    Json(credential_configuration): Json<CredentialConfiguration>,
) -> Result<Response, ApiError> {
    let command = ServerConfigCommand::UpdateCredentialConfiguration {
        credential_configuration: credential_configuration.clone(),
        provisioned: false,
    };

    command_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        SERVER_CONFIG_ID,
        &state.command.server_config,
        command,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(credential_configuration)).into_response())
}
