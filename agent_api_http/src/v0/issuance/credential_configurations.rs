use crate::handlers::command_handler;
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
        actor.clone().and_then(|Extension(actor)| actor),
        SERVER_CONFIG_ID,
        &state.command.server_config,
        command,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(credential_configuration)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_issuance::services::IssuanceServices;
    use agent_secret_manager::service::Service;
    use agent_store::{in_memory::InMemory, issuance_state};

    #[serial_test::serial]
    #[tokio::test]
    async fn credential_configurations_dispatches_update_command() {
        let state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        agent_issuance::state::initialize(&state).await.unwrap();
        let credential_configuration = serde_json::from_value(serde_json::json!({
            "credential_configuration_id": "test",
            "format": "jwt_vc_json",
            "type": ["VerifiableCredential"]
        }))
        .unwrap();

        let response = credential_configurations(State(state), None, Json(credential_configuration))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
    }
}
