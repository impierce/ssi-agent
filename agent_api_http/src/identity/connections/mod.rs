use std::sync::Arc;

use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_identity::{connection::command::ConnectionCommand, state::IdentityState};
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
pub struct PostConnectionsEndpointRequest {
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub domain: Option<Url>,
    #[serde(default)]
    pub dids: Vec<DIDUrl>,
    #[serde(default)]
    pub credential_offer_endpoint: Option<Url>,
}

#[axum_macros::debug_handler]
pub(crate) async fn post_connections(
    State(state): State<Arc<IdentityState>>,
    Json(PostConnectionsEndpointRequest {
        alias,
        domain,
        dids,
        credential_offer_endpoint,
    }): Json<PostConnectionsEndpointRequest>,
) -> Result<Response, ApiError> {
    let connection_id = uuid::Uuid::new_v4().to_string();

    let command = ConnectionCommand::AddConnection {
        connection_id: connection_id.clone(),
        alias,
        domain,
        dids,
        credential_offer_endpoint,
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

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConnectionsEndpointRequest {
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub domain: Option<Url>,
    #[serde(default)]
    pub did: Option<DIDUrl>,
}

#[axum_macros::debug_handler]
pub(crate) async fn get_connections(
    State(state): State<Arc<IdentityState>>,
    Form(GetConnectionsEndpointRequest { alias, domain, did }): Form<GetConnectionsEndpointRequest>,
) -> Result<Response, ApiError> {
    debug!("Request Params - alias: {alias:?}, domain: {domain:?}, did: {did:?}");

    let filtered_connections = query_handler("all_connections", &state.query.all_connections)
        .await?
        .map(|all_connections_view| {
            let filtered_connections: Vec<_> = all_connections_view
                .connections
                .into_values()
                .filter(|connection| {
                    alias
                        .as_ref()
                        .map_or(true, |alias| connection.alias.as_ref() == Some(alias))
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
