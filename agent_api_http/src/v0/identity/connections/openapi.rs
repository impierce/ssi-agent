use crate::v0::identity::connections::{
    __path_accept_connection_changes, __path_get_connection, __path_get_connections, __path_remove_connection,
    __path_sync_connection,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(get_connections, get_connection, sync_connection, accept_connection_changes, remove_connection),
    tags(
        (name = "Connections", description = "Manage trusted connections."),
    )
)]
pub(crate) struct ConnectionsApi;
