use crate::error::IntoApiErrorExt;
use crate::extractors::RequestActor;
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
        (status = 400, description = "Malformed JSON request body"),
        (status = 409, description = "The profile was provisioned through configuration and cannot be modified at runtime"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn patch_profile(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
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
            actor.clone(),
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
            actor.clone(),
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
            actor.clone(),
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
            actor.clone(),
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
    RequestActor(actor): RequestActor,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        PROFILE_ID,
        Some(PROFILE_ID),
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_identity::services::IdentityServices;
    use agent_shared::{config::config, handlers::command_handler as internal_command_handler};
    use agent_store::{identity_state, in_memory::InMemory};
    use axum::{
        body::{to_bytes, Body},
        extract::Request,
        Router,
    };
    use serde_json::{json, Value};
    use shared_kernel::authorization::Caller;
    use tower::ServiceExt;

    async fn setup() -> (Arc<IdentityState>, Router) {
        let state = Arc::new(
            identity_state(
                &InMemory,
                IdentityServices::default(),
                &shared_kernel::EventBusHandle::default(),
                vec![],
            )
            .await,
        );
        let app = crate::v0::identity::router(state.clone());

        (state, app)
    }

    /// Creates a profile that mirrors the configured display, so that `query_profile` leaves the global configuration
    /// unchanged for the other tests in this binary.
    async fn create_profile(state: &IdentityState, source: Source) {
        let display = config().display.first().cloned().unwrap();

        internal_command_handler(
            state.authorization_checker.clone(),
            Caller::Internal,
            PROFILE_ID,
            &state.command.profile,
            ProfileCommand::CreateProfile {
                profile_id: PROFILE_ID.to_string(),
                display_name: Some(display.name),
                description: display.description,
                logo: display.logo,
                country: None,
                source,
            },
        )
        .await
        .unwrap();
    }

    async fn get(app: &Router) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(Request::get("/v0/profile").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    async fn patch(app: &Router, body: Value) -> StatusCode {
        app.clone()
            .oneshot(
                Request::patch("/v0/profile")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn get_profile_returns_not_found_without_a_profile() {
        let (_state, app) = setup().await;

        let (status, _) = get(&app).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn patch_profile_updates_every_given_field() {
        let (state, app) = setup().await;
        create_profile(&state, Source::Default).await;
        let display = config().display.first().cloned().unwrap();

        let status = patch(
            &app,
            json!({
                "displayName": display.name,
                "description": null,
                "logo": display.logo,
                "country": "NL",
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, profile) = get(&app).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            profile,
            json!({
                "displayName": display.name,
                "logo": display.logo,
                "country": "NL",
                "source": "Runtime",
            })
        );
    }

    #[tokio::test]
    async fn patch_profile_without_fields_leaves_the_profile_unchanged() {
        let (state, app) = setup().await;
        create_profile(&state, Source::Default).await;

        assert_eq!(patch(&app, json!({})).await, StatusCode::OK);

        let (_, profile) = get(&app).await;
        assert_eq!(profile["source"], "Default");
    }

    #[tokio::test]
    async fn patch_profile_is_rejected_for_a_provisioned_profile() {
        let (state, app) = setup().await;
        create_profile(&state, Source::Provisioned).await;

        let status = patch(&app, json!({ "country": "NL" })).await;
        assert_eq!(status, StatusCode::CONFLICT);

        let (_, profile) = get(&app).await;
        assert_eq!(profile["source"], "Provisioned");
        assert!(profile.get("country").is_none());
    }
}
