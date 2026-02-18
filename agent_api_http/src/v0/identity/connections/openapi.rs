use crate::v0::identity::connections::{__path_get_connection, __path_get_connections};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(get_connections, get_connection),
    tags(
        (name = "connections", description = "Manage trusted connections."),
    )
)]
pub(crate) struct ConnectionsApi;
