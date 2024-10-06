use crate::API_VERSION;
use agent_identity::{connection::command::ConnectionCommand, state::IdentityState};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Form, Json,
};
use hyper::{header, StatusCode};
use identity_core::common::Url;
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use tracing::info;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostConnectionsEndpointRequest {
    #[serde(default)]
    pub domain: Option<Url>,
    #[serde(default)]
    pub dids: Vec<DIDUrl>,
    #[serde(default)]
    pub credential_offer_endpoint: Option<Url>,
}

#[axum_macros::debug_handler]
pub(crate) async fn post_connections(
    State(state): State<IdentityState>,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    // TODO: implement a body consuming extractor that logs the body so that we don't need to log it in each handler.
    // This way we can also immediately deserialize the body here into a typed struct instead of deserializing into a
    // `serde_json::Value` first. See:
    // https://github.com/tokio-rs/axum/blob/main/examples/consume-body-in-extractor-or-middleware/src/main.rs
    info!("Request Body: {}", payload);

    let Ok(PostConnectionsEndpointRequest {
        domain,
        dids,
        credential_offer_endpoint,
    }) = serde_json::from_value(payload)
    else {
        return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
    };

    let connection_id = uuid::Uuid::new_v4().to_string();

    let command = ConnectionCommand::AddConnection {
        connection_id: connection_id.clone(),
        domain,
        dids,
        credential_offer_endpoint,
    };

    if command_handler(&connection_id, &state.command.connection, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Return the connection.
    match query_handler(&connection_id, &state.query.connection).await {
        Ok(Some(connection)) => (
            StatusCode::CREATED,
            [(header::LOCATION, &format!("{API_VERSION}/connections/{connection_id}"))],
            Json(connection),
        )
            .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConnectionsEndpointRequest {
    #[serde(default)]
    pub domain: Option<Url>,
    #[serde(default)]
    pub did: Option<DIDUrl>,
}

#[axum_macros::debug_handler]
pub(crate) async fn get_connections(
    State(state): State<IdentityState>,
    Form(GetConnectionsEndpointRequest { domain, did }): Form<GetConnectionsEndpointRequest>,
) -> Response {
    info!("Request Params - domain: {:?}, did: {:?}", domain, did);

    match query_handler("all_connections", &state.query.all_connections).await {
        Ok(Some(all_connections_view)) => {
            let filtered_connections: HashMap<_, _> = all_connections_view
                .connections
                .into_iter()
                .filter(|(_, connection)| {
                    domain
                        .as_ref()
                        .map_or(true, |domain| connection.domain.as_ref() == Some(domain))
                        && did.as_ref().map_or(true, |did| connection.dids.contains(did))
                })
                .collect();

            (StatusCode::OK, Json(filtered_connections)).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!({}))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[axum_macros::debug_handler]
pub(crate) async fn get_connection(State(state): State<IdentityState>, Path(connection_id): Path<String>) -> Response {
    match query_handler(&connection_id, &state.query.connection).await {
        Ok(Some(connection_view)) => (StatusCode::OK, Json(connection_view)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
