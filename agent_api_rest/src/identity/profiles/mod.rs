use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_identity::profile::command::ProfileCommand;
use agent_identity::state::PROFILE_ID;
use agent_identity::{connection::command::ConnectionCommand, state::IdentityState};
use agent_shared::config::{config_mut, Logo};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Form, Json,
};
use http_api_problem::ApiError;
use hyper::{header, StatusCode};
use identity_core::common::Url;
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostProfilesEndpointRequest {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub logo: Option<Logo>,
}

#[axum_macros::debug_handler]
pub(crate) async fn post_profiles(
    State(state): State<IdentityState>,
    Json(PostProfilesEndpointRequest { display_name, logo }): Json<PostProfilesEndpointRequest>,
) -> Result<Response, ApiError> {
    let profile_id = PROFILE_ID.to_string();

    let command = ProfileCommand::CreateProfile {
        profile_id: profile_id.clone(),
        display_name,
        logo,
        provisioned: None,
    };

    command_handler(&profile_id, &state.command.profile, command).await?;

    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, &format!("{API_VERSION}/profiles/{profile_id}"))],
    )
        .into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn get_profile(
    State(state): State<IdentityState>,
    Path(profile_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&profile_id, &state.query.profile)
        .await?
        .map(|profile_view| (StatusCode::OK, Json(profile_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
