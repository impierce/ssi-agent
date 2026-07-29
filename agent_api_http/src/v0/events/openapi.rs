use crate::v0::events::__path_events_sse_handler;
use shared_kernel::event_bus::CloudEvent;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(events_sse_handler),
    components(schemas(CloudEvent)),
    tags(
        (name = "Events", description = "Stream domain events as CloudEvents via SSE.")
    )
)]
pub struct EventsApi;
