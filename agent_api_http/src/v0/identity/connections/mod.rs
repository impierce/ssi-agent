use std::sync::Arc;

use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_identity::{
    connection::aggregate::{Connection, DisplayProperties},
    connection::command::ConnectionCommand,
    state::IdentityState,
};
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

pub mod openapi;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostConnectionsEndpointRequest {
    #[serde(default)]
    pub domain: Option<Url>,
}

#[axum_macros::debug_handler]
pub(crate) async fn post_connections(
    State(state): State<Arc<IdentityState>>,
    Json(PostConnectionsEndpointRequest { domain }): Json<PostConnectionsEndpointRequest>,
) -> Result<Response, ApiError> {
    let connection_id = uuid::Uuid::new_v4().to_string();

    let command = ConnectionCommand::AddConnection {
        connection_id: connection_id.clone(),
        domain,
    };

    command_handler(&connection_id, &state.command.connection, command).await?;

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
    pub display: Option<DisplayProperties>,
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub domain: Option<Url>,
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
        (status = 200, description = "All connections retrieved successfully", body = [Connection])
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_connections(
    State(state): State<Arc<IdentityState>>,
    Form(GetConnectionsEndpointRequest { display, domain, did }): Form<GetConnectionsEndpointRequest>,
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
                        && domain
                            .as_ref()
                            .map_or(true, |domain| connection.domain.as_ref() == Some(domain))
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
    path = "/connections/{connection_id}",
    operation_id = "get_connection_by_id",
    tags = ["Connections"],
    responses(
        (status = 200, description = "Connection retrieved successfully", body = Connection)
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_connection(
    State(state): State<Arc<IdentityState>>,
    Path(connection_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&connection_id, &state.query.connection)
        .await?
        .map(|connection_view| (StatusCode::OK, Json(connection_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

/// Sync connection by ID
///
/// Sync the latest version of a connection by its unique identifier.
#[utoipa::path(
    post,
    path = "/connections/sync/{connection_id}",
    operation_id = "sync_connection_by_id",
    tags = ["Connections"],
    responses(
        (status = 200, description = "Connection synced successfully", body = Connection)
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn sync_connection(
    State(state): State<Arc<IdentityState>>,
    Path(connection_id): Path<String>,
) -> Result<Response, ApiError> {
    let command = ConnectionCommand::SyncConnection {
        connection_id: connection_id.clone(),
    };
    command_handler(&connection_id, &state.command.connection, command).await?;
    Ok(StatusCode::OK.into_response())
}

pub(crate) async fn accept_connection_changes(
    State(state): State<Arc<IdentityState>>,
    Path(connection_id): Path<String>,
) -> Result<Response, ApiError> {
    let command = ConnectionCommand::AcceptConnectionChanges {
        connection_id: connection_id.clone(),
    };
    command_handler(&connection_id, &state.command.connection, command).await?;
    Ok(StatusCode::OK.into_response())
}

pub(crate) async fn reject_connection_changes(
    State(state): State<Arc<IdentityState>>,
    Path(connection_id): Path<String>,
) -> Result<Response, ApiError> {
    let command = ConnectionCommand::RejectConnectionChanges {
        connection_id: connection_id.clone(),
    };
    command_handler(&connection_id, &state.command.connection, command).await?;
    Ok(StatusCode::OK.into_response())
}
