use crate::handlers::{command_handler, query_handler};
use agent_issuance::server_config::command::ServerConfigCommand;
use agent_issuance::state::{IssuanceState, SERVER_CONFIG_ID};
use agent_shared::config::CredentialConfiguration;
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;

#[axum_macros::debug_handler]
pub(crate) async fn credential_configuration(
    State(state): State<IssuanceState>,
    Path(credential_configuration_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(SERVER_CONFIG_ID, &state.query.server_config)
        .await?
        .and_then(|server_config_view| {
            server_config_view
                .credential_issuer_metadata
                .as_ref()
                .and_then(|credential_issuer_metadata| {
                    credential_issuer_metadata
                        .credential_configurations_supported
                        .get(&credential_configuration_id)
                        .cloned()
                })
        })
        .map(|credential_configuration| (StatusCode::OK, Json(credential_configuration)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[axum_macros::debug_handler]
pub(crate) async fn credential_configurations(
    State(state): State<IssuanceState>,
    Json(credential_configuration): Json<CredentialConfiguration>,
) -> Result<Response, ApiError> {
    let command = ServerConfigCommand::AddCredentialConfiguration {
        credential_configuration: credential_configuration.clone(),
    };

    command_handler(SERVER_CONFIG_ID, &state.command.server_config, command).await?;

    // FIXME: This should be a 201 Created response with the location header set to the URL of the created resource.
    let response = (StatusCode::CREATED, Json(credential_configuration)).into_response();
    Ok(response)
}

#[axum_macros::debug_handler]
pub(crate) async fn all_credential_configurations(State(state): State<IssuanceState>) -> Result<Response, ApiError> {
    let all_credential_configurations = query_handler(SERVER_CONFIG_ID, &state.query.server_config)
        .await?
        .map(|server_config_view| {
            server_config_view
                .credential_issuer_metadata
                .as_ref()
                .map(|credential_issuer_metadata| {
                    credential_issuer_metadata.credential_configurations_supported.clone()
                })
        })
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_credential_configurations)).into_response())
}
