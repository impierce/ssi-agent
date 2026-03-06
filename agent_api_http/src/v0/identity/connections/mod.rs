use std::sync::Arc;

use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_identity::connection::views::all_connections::AllConnectionsView;
use agent_identity::connection::views::ConnectionView;
use agent_identity::{
    connection::aggregate::ConnectionDisplayProperties, connection::command::ConnectionCommand, state::IdentityState,
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

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddConnectionEndpointRequest {
    #[serde(default)]
    pub domain: String,
}

/// Add a Connection
///
/// Adds a new connection based on the provided domain.
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
    Json(AddConnectionEndpointRequest { domain }): Json<AddConnectionEndpointRequest>,
) -> Result<Response, ApiError> {
    let connection_id = uuid::Uuid::new_v4().to_string();
    let normalized = if !domain.starts_with("http://") && !domain.starts_with("https://") {
        format!("https://{domain}")
    } else {
        domain
    };
    let domain: Url = Url::parse(&normalized)
        .map_err(|e| ApiError::builder(StatusCode::BAD_REQUEST).message(format!("Invalid domain: {e}")))?;

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
    pub display: Option<ConnectionDisplayProperties>,
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
        (status = 200, description = "All connections retrieved successfully", body = [ConnectionView])
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
    path = "/connections/{id}",
    operation_id = "get_connection_by_id",
    tags = ["Connections"],
    responses(
        (status = 200)
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_connection(
    State(state): State<Arc<IdentityState>>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&id, &state.query.connection)
        .await?
        .map(|connection_view| (StatusCode::OK, Json(connection_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncConnectionRequest {
    #[serde(default)]
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
    Json(SyncConnectionRequest { id }): Json<SyncConnectionRequest>,
) -> Result<Response, ApiError> {
    let command = ConnectionCommand::SyncConnection {
        connection_id: id.clone(),
    };
    command_handler(&id, &state.command.connection, command).await?;
    Ok(StatusCode::OK.into_response())
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcceptConnectionChangesRequest {
    #[serde(default)]
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
    Json(AcceptConnectionChangesRequest { id }): Json<AcceptConnectionChangesRequest>,
) -> Result<Response, ApiError> {
    let command = ConnectionCommand::AcceptConnectionChanges {
        connection_id: id.clone(),
    };
    command_handler(&id, &state.command.connection, command).await?;
    Ok(StatusCode::OK.into_response())
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoveConnectionRequest {
    #[serde(default)]
    id: String,
}
/// Remove Connection
///
/// Removes a connection
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
    Json(RemoveConnectionRequest { id }): Json<RemoveConnectionRequest>,
) -> Result<Response, ApiError> {
    let command = ConnectionCommand::RemoveConnection {
        connection_id: id.clone(),
    };
    command_handler(&id, &state.command.connection, command).await?;
    Ok(StatusCode::OK.into_response())
}
