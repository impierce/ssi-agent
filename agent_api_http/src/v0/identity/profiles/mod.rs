use crate::error::IntoApiErrorExt;
use crate::handlers::{command_handler, query_handler, request_actor};
use crate::utils::serde_explicit_null;
use agent_identity::profile::aggregate::Source;
use agent_identity::profile::command::ProfileCommand;
use agent_identity::state::IdentityState;
use agent_identity::state::{query_profile, PROFILE_ID};
use agent_shared::config::Logo;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Extension, Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use shared_kernel::authorization::Actor;
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
    actor: Option<Extension<Option<Actor>>>,
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
            request_actor(&actor),
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
            request_actor(&actor),
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
            request_actor(&actor),
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
            request_actor(&actor),
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
pub(crate) async fn get_profile(
    State(state): State<Arc<IdentityState>>,
    actor: Option<Extension<Option<Actor>>>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        PROFILE_ID,
        &state.query.profile,
    )
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
