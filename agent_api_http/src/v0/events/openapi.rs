use crate::v0::events::__path_events_sse_handler;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(events_sse_handler),
    tags(
        (name = "Events", description = "Stream domain events as CloudEvents via SSE.")
    )
)]
pub struct EventsApi;
