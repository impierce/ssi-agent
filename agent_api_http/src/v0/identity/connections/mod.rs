use shared_kernel::authorization::Actor;
use std::sync::Arc;

use crate::handlers::{command_handler, query_handler, request_actor};
use crate::API_VERSION;
use agent_identity::{
    connection::{aggregate::ConnectionDisplayProperties, command::ConnectionCommand, views::ConnectionView},
    state::IdentityState,
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Extension, Form, Json,
};
use http_api_problem::ApiError;
use hyper::{header, StatusCode};
use identity_core::common::Url;
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};

pub mod openapi;

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddConnectionEndpointRequest {
    pub url: String,
}

/// Add a Connection
///
/// Adds a new connection based on the provided url.
#[utoipa::path(
    post,
    path = "/connections",
    operation_id = "add_connection",
    tags = ["Connections"],
    responses(
        (status = 201, description = "Connection added successfully", body = ConnectionView,
            headers(
                ("Location" = String, description = "URI of the newly created connection")
            )
        ),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn post_connection(
    State(state): State<Arc<IdentityState>>,
    actor: Option<Extension<Option<Actor>>>,
    Json(AddConnectionEndpointRequest { url }): Json<AddConnectionEndpointRequest>,
) -> Result<Response, ApiError> {
    let connection_id = uuid::Uuid::new_v4().to_string();

    let url = parse_url(&url)?;
    let command = ConnectionCommand::AddConnection {
        connection_id: connection_id.clone(),
        url,
    };

    command_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &connection_id,
        &state.command.connection,
        command,
    )
    .await?;

    // Return the connection.
    query_handler(&connection_id, &state.query.connection)
        .await?
        .map(|connection_view| {
            (
                StatusCode::CREATED,
                [(header::LOCATION, &format!("{API_VERSION}/connections/{connection_id}"))],
                Json(connection_view),
            )
                .into_response()
        })
        // TODO: this *should* be an impossible error, what should we return here?
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetConnectionsEndpointRequest {
    #[serde(default)]
    pub display: Option<ConnectionDisplayProperties>,
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub url: Option<Url>,
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub did: Option<DIDUrl>,
}

/// List all connections
///
/// List all available connections.
#[utoipa::path(
    get,
    path = "/connections",
    operation_id = "get_all_connections",
    tags = ["Connections"],
    responses(
        (status = 200, description = "All connections retrieved successfully", body = [ConnectionView])
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_connections(
    State(state): State<Arc<IdentityState>>,
    _actor: Option<Extension<Option<Actor>>>,
    Form(GetConnectionsEndpointRequest { display, url, did }): Form<GetConnectionsEndpointRequest>,
) -> Result<Response, ApiError> {
    let filtered_connections = query_handler("all_connections", &state.query.all_connections)
        .await?
        .map(|all_connections_view| {
            let filtered_connections: Vec<_> = all_connections_view
                .connections
                .into_values()
                .filter(|connection| {
                    display
                        .as_ref()
                        .map_or(true, |display| connection.display.as_ref() == Some(display))
                        && url.as_ref().map_or(true, |url| *url == connection.url)
                        && did.as_ref().map_or(true, |did| connection.dids.contains(did))
                })
                .collect();

            filtered_connections
        })
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(filtered_connections)).into_response())
}

/// Get connection by ID
///
/// Retrieve a specific connection by its unique identifier.
#[utoipa::path(
    get,
    path = "/connections/{id}",
    operation_id = "get_connection_by_id",
    tags = ["Connections"],
    responses(
        (status = 200, description = "Connection retrieved successfully", body = ConnectionView),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_connection(
    State(state): State<Arc<IdentityState>>,
    _actor: Option<Extension<Option<Actor>>>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&id, &state.query.connection)
        .await?
        .filter(|view| !view.deleted)
        .map(|connection_view| (StatusCode::OK, Json(connection_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncConnectionRequest {
    id: String,
}

/// Sync connection by ID
///
/// Sync the latest version of a connection by its unique identifier.
#[utoipa::path(
    post,
    path = "/connections/sync-connection",
    operation_id = "sync_connection_by_id",
    tags = ["Connections"],
    responses(
        (status = 200)
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn sync_connection(
    State(state): State<Arc<IdentityState>>,
    actor: Option<Extension<Option<Actor>>>,
    Json(SyncConnectionRequest { id }): Json<SyncConnectionRequest>,
) -> Result<Response, ApiError> {
    let command = ConnectionCommand::SyncConnection {
        connection_id: id.clone(),
    };
    command_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &id,
        &state.command.connection,
        command,
    )
    .await?;
    Ok(StatusCode::OK.into_response())
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcceptConnectionChangesRequest {
    id: String,
}

/// Accept Pending Changes
///
/// Accept pending changes to a connection.
#[utoipa::path(
    post,
    path = "/connections/accept-pending-changes",
    operation_id = "accept_connection_changes",
    tags = ["Connections"],
    responses(
        (status = 200)
    )
)]
pub(crate) async fn accept_connection_changes(
    State(state): State<Arc<IdentityState>>,
    actor: Option<Extension<Option<Actor>>>,
    Json(AcceptConnectionChangesRequest { id }): Json<AcceptConnectionChangesRequest>,
) -> Result<Response, ApiError> {
    let command = ConnectionCommand::AcceptConnectionChanges {
        connection_id: id.clone(),
    };
    command_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &id,
        &state.command.connection,
        command,
    )
    .await?;
    Ok(StatusCode::OK.into_response())
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoveConnectionRequest {
    id: String,
}

/// Remove Connection
///
/// Removes a connection by its ID.
#[utoipa::path(
    post,
    path = "/connections/remove-connection",
    operation_id = "remove_connection",
    tags = ["Connections"],
    responses(
        (status = 200)
    )
)]
pub(crate) async fn remove_connection(
    State(state): State<Arc<IdentityState>>,
    actor: Option<Extension<Option<Actor>>>,
    Json(RemoveConnectionRequest { id }): Json<RemoveConnectionRequest>,
) -> Result<Response, ApiError> {
    let command = ConnectionCommand::RemoveConnection {
        connection_id: id.clone(),
    };
    command_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &id,
        &state.command.connection,
        command,
    )
    .await?;
    Ok(StatusCode::OK.into_response())
}

// HELPERS
#[allow(clippy::result_large_err)]
pub fn parse_url(input: &str) -> Result<Url, ApiError> {
    let input = input.trim();
    let with_scheme = match input.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None if input.starts_with("https://") => input.to_string(),
        None => format!("https://{input}"),
    };

    let url = Url::parse(&with_scheme).map_err(|e| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .message(format!("Invalid issuer URL: {e}"))
            .finish()
    })?;

    let host = url.host_str().ok_or_else(|| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .message("Url missing host".to_string())
            .finish()
    })?;

    if !host.contains('.') {
        return Err(ApiError::builder(StatusCode::BAD_REQUEST)
            .message("Url must contain a top-level domain (e.g. .com, .nl, .eu).".to_string())
            .finish());
    }

    Ok(url)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_parsing_with_http_prefix() {
        let input_string = "http://a-via-lactea.example.com/";
        let parsed = parse_url(input_string).unwrap();

        assert_eq!(parsed, Url::parse("https://a-via-lactea.example.com/").unwrap());
    }

    #[test]
    fn test_parsing_with_no_prefix() {
        let input_string = "a-via-lactea.example.com/";
        let parsed = parse_url(input_string).unwrap();

        assert_eq!(parsed, Url::parse("https://a-via-lactea.example.com/").unwrap());
    }

    #[test]
    fn test_parsing_www() {
        let input_string = "www.a-via-lactea.example.com/";
        let parsed = parse_url(input_string).unwrap();

        assert_eq!(parsed, Url::parse("https://www.a-via-lactea.example.com/").unwrap());
    }

    #[test]
    fn test_parsing_already_https() {
        let input_string = "https://a-via-lactea.example.com/";
        let parsed = parse_url(input_string).unwrap();

        assert_eq!(parsed, Url::parse("https://a-via-lactea.example.com/").unwrap());
    }

    #[test]
    fn invalid_input_no_tld() {
        let input_string = "a-via-lactea";
        assert!(parse_url(input_string).is_err());
    }
}
