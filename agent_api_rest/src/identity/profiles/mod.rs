use crate::error::IntoApiErrorExt;
use crate::handlers::{command_handler, query_handler};
use crate::utils::serde_explicit_null;
use agent_identity::profile::aggregate::Source;
use agent_identity::profile::command::ProfileCommand;
use agent_identity::state::IdentityState;
use agent_identity::state::{query_profile, PROFILE_ID};
use agent_shared::config::Logo;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchProfileEndpointRequest {
    #[serde(default, with = "serde_explicit_null")]
    pub display_name: Option<Option<String>>,
    #[serde(default, with = "serde_explicit_null")]
    pub logo: Option<Option<Logo>>,
    #[serde(default, with = "serde_explicit_null")]
    pub country: Option<Option<String>>,
}

#[axum_macros::debug_handler]
pub(crate) async fn patch_profile(
    State(state): State<IdentityState>,
    Json(PatchProfileEndpointRequest {
        display_name,
        logo,
        country,
    }): Json<PatchProfileEndpointRequest>,
) -> Result<Response, ApiError> {
    let profile_id = PROFILE_ID.to_string();

    if let Some(display_name) = display_name {
        let command = ProfileCommand::UpdateDisplayName {
            display_name: display_name.unwrap_or_default(),
            source: Source::Runtime,
        };

        command_handler(&profile_id, &state.command.profile, command).await?;
    }

    if let Some(logo) = logo {
        let command = ProfileCommand::UpdateLogo {
            logo,
            source: Source::Runtime,
        };

        command_handler(&profile_id, &state.command.profile, command).await?;
    }

    if let Some(country) = country {
        let command = ProfileCommand::UpdateCountry {
            country,
            source: Source::Runtime,
        };

        command_handler(&profile_id, &state.command.profile, command).await?;
    }

    query_profile(&state).await.map_err(IntoApiErrorExt::into_api_error)?;

    Ok(StatusCode::OK.into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn get_profile(State(state): State<IdentityState>) -> Result<Response, ApiError> {
    query_handler(PROFILE_ID, &state.query.profile)
        .await?
        .map(|profile_view| (StatusCode::OK, Json(profile_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
