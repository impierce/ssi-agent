use crate::v0::identity::connections::{
    __path_accept_connection_changes, __path_get_connection, __path_get_connections, __path_post_connection,
    __path_remove_connection, __path_sync_connection,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(accept_connection_changes, get_connection, get_connections, post_connection, remove_connection, sync_connection),
    tags(
        (name = "Connections", description = "Manage trusted connections."),
    )
)]
pub(crate) struct ConnectionsApi;
