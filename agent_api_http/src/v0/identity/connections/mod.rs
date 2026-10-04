use std::sync::Arc;

use crate::extractors::RequestActor;
use crate::handlers::{command_handler, internal_query_handler, query_handler};
use crate::API_VERSION;
use agent_identity::{
    connection::{aggregate::ConnectionDisplayProperties, command::ConnectionCommand, views::ConnectionView},
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
        (status = 400, description = "Malformed JSON request body"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn post_connection(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
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
        actor.clone(),
        &connection_id,
        &state.command.connection,
        command,
    )
    .await?;

    // Return the connection.
    internal_query_handler(
        state.authorization_checker.clone(),
        &connection_id,
        Some(&connection_id),
        &state.query.connection,
    )
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
        (status = 200, description = "All connections retrieved successfully", body = [ConnectionView]),
        (status = 400, description = "Invalid query parameter"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_connections(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Form(GetConnectionsEndpointRequest { display, url, did }): Form<GetConnectionsEndpointRequest>,
) -> Result<Response, ApiError> {
    let filtered_connections = query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        "all_connections",
        None,
        &state.query.all_connections,
    )
    .await?
    .map(|all_connections_view| {
        let filtered_connections: Vec<_> = crate::utils::newest_first(all_connections_view.connections)
            .filter(|connection| {
                display
                    .as_ref()
                    .is_none_or(|display| connection.display.as_ref() == Some(display))
                    && url.as_ref().is_none_or(|url| *url == connection.url)
                    && did.as_ref().is_none_or(|did| connection.dids.contains(did))
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
        (status = 200, description = "Connection retrieved successfully", body = ConnectionView),
        (status = 400, description = "Invalid path parameter"),
        (status = 404, description = "Connection not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_connection(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &id,
        Some(&id),
        &state.query.connection,
    )
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
        (status = 200),
        (status = 400, description = "Malformed JSON request body"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn sync_connection(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Json(SyncConnectionRequest { id }): Json<SyncConnectionRequest>,
) -> Result<Response, ApiError> {
    let command = ConnectionCommand::SyncConnection {
        connection_id: id.clone(),
    };
    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
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
        (status = 200),
        (status = 400, description = "Malformed JSON request body"),
        (status = 404, description = "Connection not found"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
pub(crate) async fn accept_connection_changes(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Json(AcceptConnectionChangesRequest { id }): Json<AcceptConnectionChangesRequest>,
) -> Result<Response, ApiError> {
    let command = ConnectionCommand::AcceptConnectionChanges {
        connection_id: id.clone(),
    };
    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
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
        (status = 200),
        (status = 400, description = "Malformed JSON request body"),
        (status = 404, description = "Connection not found"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
pub(crate) async fn remove_connection(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Json(RemoveConnectionRequest { id }): Json<RemoveConnectionRequest>,
) -> Result<Response, ApiError> {
    let command = ConnectionCommand::RemoveConnection {
        connection_id: id.clone(),
    };
    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
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
        #[cfg(not(feature = "allow-localhost"))]
        Some(rest) => format!("https://{rest}"),
        #[cfg(feature = "allow-localhost")]
        Some(_rest) => input.to_string(),
        None if input.starts_with("https://") => input.to_string(),
        #[cfg(feature = "allow-localhost")]
        None if input.starts_with("localhost") => format!("http://{input}"),
        None => format!("https://{input}"),
    };

    let url = Url::parse(&with_scheme).map_err(|e| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .message(format!("Invalid issuer URL: {e}"))
            .finish()
    })?;

    #[cfg(not(feature = "allow-localhost"))]
    {
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
    }

    Ok(url)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use agent_identity::services::IdentityServices;
    use agent_store::{identity_state, in_memory::InMemory};
    use cqrs_es::persist::ViewContext;

    #[test]
    #[cfg(not(feature = "allow-localhost"))]
    fn test_parsing_with_http_prefix_upgrades_to_https() {
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

    #[tokio::test]
    async fn removed_connection_stays_hidden_after_repository_round_trip() {
        let event_bus = shared_kernel::EventBusHandle::default();
        let state = Arc::new(identity_state(&InMemory, IdentityServices::default(), &event_bus).await);
        let connection_id = "removed-connection";

        state
            .query
            .connection
            .update_view(
                ConnectionView {
                    connection_id: connection_id.to_string(),
                    deleted: true,
                    ..Default::default()
                },
                ViewContext::new(connection_id.to_string(), 0),
            )
            .await
            .unwrap();

        let response = get_connection(State(state), RequestActor(None), Path(connection_id.to_string()))
            .await
            .unwrap_err()
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    mod endpoints {
        use super::*;
        use agent_shared::handlers::command_handler as internal_command_handler;
        use axum::{
            body::{to_bytes, Body},
            extract::Request,
            Router,
        };
        use serde_json::{json, Value};
        use shared_kernel::authorization::Caller;
        use tower::ServiceExt;
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        const CONNECTION_ID: &str = "connection-1";

        async fn mock_issuer(name: &str) -> MockServer {
            let mock_server = MockServer::start().await;
            mount_metadata(&mock_server, name).await;
            mock_server
        }

        async fn mount_metadata(mock_server: &MockServer, name: &str) {
            mock_server.reset().await;
            Mock::given(method("GET"))
                .and(path("/.well-known/openid-credential-issuer"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "credential_issuer": mock_server.uri(),
                    "credential_endpoint": format!("{}/credentials", mock_server.uri()),
                    "display": [{ "name": name, "locale": "en" }],
                    "credential_configurations_supported": {}
                })))
                .mount(mock_server)
                .await;
        }

        /// Adds a connection through the command handler, since `parse_url` only accepts the mock issuer's
        /// `http://127.0.0.1` address with the `allow-localhost` feature.
        async fn setup(mock_server: &MockServer) -> Router {
            let state =
                Arc::new(identity_state(&InMemory, IdentityServices::default(), &Default::default(), vec![]).await);

            internal_command_handler(
                state.authorization_checker.clone(),
                Caller::Internal,
                CONNECTION_ID,
                &state.command.connection,
                ConnectionCommand::AddConnection {
                    connection_id: CONNECTION_ID.to_string(),
                    url: mock_server.uri().parse().unwrap(),
                },
            )
            .await
            .unwrap();

            crate::v0::identity::router(state)
        }

        async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
            let response = app
                .clone()
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let status = response.status();
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

            (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
        }

        async fn post(app: &Router, uri: &str, body: Value) -> StatusCode {
            app.clone()
                .oneshot(
                    Request::post(uri)
                        .header(http::header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
        }

        #[tokio::test]
        async fn connections_can_be_listed_and_filtered() {
            let mock_server = mock_issuer("Issuer").await;
            let app = setup(&mock_server).await;
            let url = format!("{}/", mock_server.uri());

            let (status, connections) = get(&app, "/v0/connections").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(connections.as_array().unwrap().len(), 1);
            assert_eq!(connections[0]["id"], CONNECTION_ID);
            assert_eq!(connections[0]["url"], url.as_str());
            assert_eq!(connections[0]["display"]["name"], "Issuer");

            let query = serde_urlencoded::to_string([("url", &url)]).unwrap();
            let (_, connections) = get(&app, &format!("/v0/connections?{query}")).await;
            assert_eq!(connections.as_array().unwrap().len(), 1);

            let (_, connections) = get(&app, "/v0/connections?url=https%3A%2F%2Fother.example.com%2F").await;
            assert_eq!(connections, json!([]));

            let (_, connections) = get(
                &app,
                "/v0/connections?did=did%3Akey%3Az6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM",
            )
            .await;
            assert_eq!(connections, json!([]));

            let (status, _) = get(&app, "/v0/connections?did=not-a-did").await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }

        #[tokio::test]
        async fn a_connection_can_be_retrieved_by_id() {
            let mock_server = mock_issuer("Issuer").await;
            let app = setup(&mock_server).await;

            let (status, connection) = get(&app, &format!("/v0/connections/{CONNECTION_ID}")).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(connection["id"], CONNECTION_ID);

            let (status, _) = get(&app, "/v0/connections/unknown-connection").await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn synced_changes_are_pending_until_accepted() {
            let mock_server = mock_issuer("Issuer").await;
            let app = setup(&mock_server).await;
            let connection_uri = format!("/v0/connections/{CONNECTION_ID}");

            mount_metadata(&mock_server, "Renamed Issuer").await;
            let status = post(&app, "/v0/connections/sync-connection", json!({ "id": CONNECTION_ID })).await;
            assert_eq!(status, StatusCode::OK);

            let (_, connection) = get(&app, &connection_uri).await;
            assert_eq!(connection["display"]["name"], "Issuer");
            assert_eq!(connection["pending_changes"]["display"]["name"], "Renamed Issuer");

            let status = post(
                &app,
                "/v0/connections/accept-pending-changes",
                json!({ "id": CONNECTION_ID }),
            )
            .await;
            assert_eq!(status, StatusCode::OK);

            let (_, connection) = get(&app, &connection_uri).await;
            assert_eq!(connection["display"]["name"], "Renamed Issuer");
            assert!(connection["pending_changes"].is_null());

            // Without pending changes, accepting is a no-op.
            let status = post(
                &app,
                "/v0/connections/accept-pending-changes",
                json!({ "id": CONNECTION_ID }),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        }

        #[tokio::test]
        async fn a_removed_connection_is_no_longer_listed() {
            let mock_server = mock_issuer("Issuer").await;
            let app = setup(&mock_server).await;

            let status = post(
                &app,
                "/v0/connections/remove-connection",
                json!({ "id": CONNECTION_ID }),
            )
            .await;
            assert_eq!(status, StatusCode::OK);

            let (status, _) = get(&app, &format!("/v0/connections/{CONNECTION_ID}")).await;
            assert_eq!(status, StatusCode::NOT_FOUND);

            let (_, connections) = get(&app, "/v0/connections").await;
            assert_eq!(connections, json!([]));

            let status = post(
                &app,
                "/v0/connections/remove-connection",
                json!({ "id": CONNECTION_ID }),
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn commands_on_unknown_connections_return_not_found() {
            let mock_server = mock_issuer("Issuer").await;
            let app = setup(&mock_server).await;

            for uri in [
                "/v0/connections/sync-connection",
                "/v0/connections/accept-pending-changes",
                "/v0/connections/remove-connection",
            ] {
                let status = post(&app, uri, json!({ "id": "unknown-connection" })).await;
                assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
            }
        }

        #[tokio::test]
        #[cfg(not(feature = "allow-localhost"))]
        async fn adding_a_connection_without_a_top_level_domain_is_rejected() {
            let mock_server = mock_issuer("Issuer").await;
            let app = setup(&mock_server).await;

            let status = post(&app, "/v0/connections", json!({ "url": "a-via-lactea" })).await;

            assert_eq!(status, StatusCode::BAD_REQUEST);
        }

        #[tokio::test]
        #[cfg(feature = "allow-localhost")]
        async fn a_connection_can_be_added() {
            let mock_server = mock_issuer("Issuer").await;
            let app = setup(&mock_server).await;

            let response = app
                .clone()
                .oneshot(
                    Request::post("/v0/connections")
                        .header(http::header::CONTENT_TYPE, "application/json")
                        .body(Body::from(json!({ "url": mock_server.uri() }).to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);

            let location = response.headers()[header::LOCATION].to_str().unwrap().to_string();
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let connection: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                location,
                format!("/v0/connections/{}", connection["id"].as_str().unwrap())
            );
            assert_eq!(connection["display"]["name"], "Issuer");

            let (_, connections) = get(&app, "/v0/connections").await;
            assert_eq!(connections.as_array().unwrap().len(), 2);
        }
    }

    #[test]
    #[cfg(not(feature = "allow-localhost"))]
    fn invalid_input_no_tld_fails() {
        let input_string = "a-via-lactea";
        assert!(parse_url(input_string).is_err());
    }

    #[cfg(feature = "allow-localhost")]
    pub mod allow_localhost_tests {
        use super::*;

        #[test]
        fn test_parsing_with_http_prefix_preserves_http() {
            let input_string = "http://a-via-lactea.example.com/";
            let parsed = parse_url(input_string).unwrap();

            assert_eq!(parsed, Url::parse("http://a-via-lactea.example.com/").unwrap());
        }

        #[test]
        fn test_parsing_localhost_defaults_to_http() {
            let input_string = "localhost:8080";
            let parsed = parse_url(input_string).unwrap();
            assert_eq!(parsed, Url::parse("http://localhost:8080/").unwrap());
        }
    }
}
