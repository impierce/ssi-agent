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

#[test]
fn test() {
    let test = PostProfilesEndpointRequest {
        display_name: Some("Test Profile".to_string()),
        logo: Some(Logo {
            uri: Some("https://example.com/logo.png".parse().unwrap()),
            alt_text: Some("test logo".to_string()),
        }),
    };

    println!("{}", serde_json::to_string_pretty(&test).unwrap());

    serde_json::json!({
        "displayName": "Test Profile",
        "logo": {
            "uri": "https://example.com/logo.png",
            "alt_text": "test logo"
        }
    });
}

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
    if let Some(display_name) = display_name {
        config_mut()
            .display
            .first_mut()
            .map(|display| display.name = display_name.clone());
    }

    if let Some(logo) = logo {
        config_mut()
            .display
            .first_mut()
            .map(|display| display.logo = Some(logo));
    }

    Ok(StatusCode::OK.into_response())

    // let profile_id = PROFILE_ID.to_string();

    // let command = ProfileCommand::CreateProfile {
    //     profile_id: profile_id.clone(),
    //     display_name,
    //     logo_uri,
    //     provisioned: false,
    // };

    // command_handler(&profile_id, &state.command.profile, command).await?;

    // // Return the connection.
    // query_handler(&connection_id, &state.query.connection)
    //     .await?
    //     .map(|connection_view| {
    //         (
    //             StatusCode::CREATED,
    //             [(header::LOCATION, &format!("{API_VERSION}/connections/{connection_id}"))],
    //             Json(connection_view),
    //         )
    //             .into_response()
    //     })
    //     // TODO: this *should* be an impossible error, what should we return here?
    //     .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[axum_macros::debug_handler]
pub(crate) async fn get_profiles(State(state): State<IdentityState>) -> Result<Response, ApiError> {
    // debug!("Request Params - alias: {alias:?}, domain: {domain:?}, did: {did:?}");

    // let filtered_connections = query_handler("all_connections", &state.query.all_connections)
    //     .await?
    //     .map(|all_connections_view| {
    //         let filtered_connections: Vec<_> = all_connections_view
    //             .connections
    //             .into_values()
    //             .filter(|connection| {
    //                 alias
    //                     .as_ref()
    //                     .map_or(true, |alias| connection.alias.as_ref() == Some(alias))
    //                     && domain
    //                         .as_ref()
    //                         .map_or(true, |domain| connection.domain.as_ref() == Some(domain))
    //                     && did.as_ref().map_or(true, |did| connection.dids.contains(did))
    //             })
    //             .collect();

    //         filtered_connections
    //     })
    //     .unwrap_or_default();

    // Ok((StatusCode::OK, Json(filtered_connections)).into_response())
    todo!()
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
