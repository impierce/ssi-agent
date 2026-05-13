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
use serde_with::skip_serializing_none;
use std::sync::Arc;

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchProfileEndpointRequest {
    #[serde(default, with = "serde_explicit_null")]
    pub display_name: Option<Option<String>>,
    #[serde(default, with = "serde_explicit_null")]
    pub description: Option<Option<String>>,
    #[serde(default, with = "serde_explicit_null")]
    pub logo: Option<Option<Logo>>,
    #[serde(default, with = "serde_explicit_null")]
    pub country: Option<Option<String>>,
}

/// Update organisation profile
///
/// Updates your organisation's profile with the given information.
#[utoipa::path(
    patch,
    path = "/profile",
    operation_id = "update_profile",
    tags = ["Identity", "Profile"],
    responses(
        (status = 200, description = "Profile updated successfully"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn patch_profile(
    State(state): State<Arc<IdentityState>>,
    Json(PatchProfileEndpointRequest {
        display_name,
        description,
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

        command_handler(
            state.authorization_checker.clone(),
            None,
            &profile_id,
            &state.command.profile,
            command,
        )
        .await?;
    }

    if let Some(description) = description {
        let command = ProfileCommand::UpdateDescription {
            description,
            source: Source::Runtime,
        };

        command_handler(
            state.authorization_checker.clone(),
            None,
            &profile_id,
            &state.command.profile,
            command,
        )
        .await?;
    }

    if let Some(logo) = logo {
        let command = ProfileCommand::UpdateLogo {
            logo,
            source: Source::Runtime,
        };

        command_handler(
            state.authorization_checker.clone(),
            None,
            &profile_id,
            &state.command.profile,
            command,
        )
        .await?;
    }

    if let Some(country) = country {
        let command = ProfileCommand::UpdateCountry {
            country,
            source: Source::Runtime,
        };

        command_handler(
            state.authorization_checker.clone(),
            None,
            &profile_id,
            &state.command.profile,
            command,
        )
        .await?;
    }

    query_profile(&state).await.map_err(IntoApiErrorExt::into_api_error)?;

    Ok(StatusCode::OK.into_response())
}

#[skip_serializing_none]
#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = Profile)]
struct GetProfileEndpointResponse {
    display_name: Option<String>,
    description: Option<String>,
    logo: Option<Logo>,
    country: Option<String>,
    source: Source,
}

/// Get organisation profile
///
/// Retrieves the profile information of your organisation.
#[utoipa::path(
    get,
    path = "/profile",
    operation_id = "get_profile",
    tags = ["Identity", "Profile"],
    responses(
        (status = 200, description = "Profile retrieved successfully", body = GetProfileEndpointResponse),
        (status = 404, description = "Profile not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_profile(State(state): State<Arc<IdentityState>>) -> Result<Response, ApiError> {
    query_handler(PROFILE_ID, &state.query.profile)
        .await?
        .map(|profile_view| {
            (
                StatusCode::OK,
                Json(GetProfileEndpointResponse {
                    display_name: profile_view.display_name,
                    description: profile_view.description,
                    logo: profile_view.logo,
                    country: profile_view.country,
                    source: profile_view.source,
                }),
            )
                .into_response()
        })
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_identity::services::IdentityServices;
    use agent_shared::{
        config::{config, set_config},
        handlers::command_handler,
    };
    use agent_store::{identity_state, in_memory::InMemory};

    #[serial_test::serial]
    #[tokio::test]
    async fn patch_profile_dispatches_all_update_commands() {
        let original = config().clone();
        let state = Arc::new(identity_state(&InMemory, IdentityServices::default(), Default::default()).await);
        command_handler(
            state.authorization_checker.clone(),
            None,
            PROFILE_ID,
            &state.command.profile,
            ProfileCommand::CreateProfile {
                profile_id: PROFILE_ID.to_string(),
                display_name: Some("Runtime Name".to_string()),
                description: None,
                logo: None,
                country: None,
                source: Source::Runtime,
            },
        )
        .await
        .unwrap();

        let response = patch_profile(
            State(state),
            Json(PatchProfileEndpointRequest {
                display_name: Some(Some("Runtime Name".to_string())),
                description: Some(Some("Runtime Description".to_string())),
                logo: Some(None),
                country: Some(Some("NL".to_string())),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        *set_config() = original;
    }
}
