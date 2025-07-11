use crate::error::IntoApiErrorExt;
use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_identity::profile::aggregate::{Profile, Source};
use agent_identity::profile::command::ProfileCommand;
use agent_identity::state::IdentityState;
use agent_identity::state::{query_profile, PROFILE_ID};
use agent_shared::config::Logo;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::{header, StatusCode};
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostProfilesEndpointRequest {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub logo: Option<Logo>,
}

#[axum_macros::debug_handler]
pub(crate) async fn put_profiles(
    State(state): State<IdentityState>,
    Json(PostProfilesEndpointRequest {
        display_name: new_display_name,
        logo: new_logo,
    }): Json<PostProfilesEndpointRequest>,
) -> Result<Response, ApiError> {
    let profile_id = PROFILE_ID.to_string();

    match query_handler(&profile_id, &state.query.profile).await? {
        Some(Profile { display_name, logo, .. }) => {
            info!("Display name: {:?}, Logo: {:?}", display_name, logo);
            info!("New display name: {:?}, New logo: {:?}", new_display_name, new_logo);

            // TODO: strictly speaking these are partial updates, so they should actually be PATCH requests.
            if new_display_name.is_some() && new_display_name != display_name {
                info!("Updating display name for profile: {}", profile_id);
                let command = ProfileCommand::UpdateDisplayName {
                    display_name: new_display_name,
                    source: Source::Runtime,
                };

                command_handler(&profile_id, &state.command.profile, command).await?;
            }

            if new_logo.is_some() && new_logo != logo {
                info!("Updating logo for profile: {}", profile_id);
                let command = ProfileCommand::UpdateLogo {
                    logo: new_logo,
                    source: Source::Runtime,
                };

                command_handler(&profile_id, &state.command.profile, command).await?;
            }

            query_profile(&state).await.map_err(IntoApiErrorExt::into_api_error)?;

            Ok((StatusCode::OK).into_response())
        }
        None => {
            info!("No display configured, creating a new one.");

            let command = ProfileCommand::CreateProfile {
                profile_id: profile_id.clone(),
                display_name: new_display_name,
                logo: new_logo,
                source: Source::Runtime,
            };

            command_handler(&profile_id, &state.command.profile, command).await?;

            query_profile(&state).await.map_err(IntoApiErrorExt::into_api_error)?;

            Ok((
                StatusCode::CREATED,
                [(header::LOCATION, &format!("{API_VERSION}/profiles/{profile_id}"))],
            )
                .into_response())
        }
    }
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
