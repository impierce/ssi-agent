use crate::v0::events::{
    __path_events_sse_handler,
    config::{
        EventPublisherConfigDto, HttpEventPublisherConfigDto, HttpEventPublisherType, NatsEventPublisherConfigDto,
        NatsEventPublisherType,
    },
    publishers::{
        __path_activate_event_publisher, __path_create_event_publisher, __path_deactivate_event_publisher,
        __path_delete_event_publisher, __path_get_event_publisher_by_id, __path_get_event_publisher_list,
        __path_update_event_publisher, ActivateEventPublisherRequest, CreateEventPublisherRequest,
        DeactivateEventPublisherRequest, DeleteEventPublisherRequest, EventPublisherDto, EventPublisherListDto,
        UpdateEventPublisherRequest,
    },
};
use bc_event_publishing::domain::event_publisher::config::{
    EventPublisherType, HttpPublisherConfig, NatsPublisherConfig,
};
use shared_kernel::event_bus::CloudEvent;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        events_sse_handler,
        create_event_publisher,
        update_event_publisher,
        activate_event_publisher,
        deactivate_event_publisher,
        delete_event_publisher,
        get_event_publisher_by_id,
        get_event_publisher_list,
    ),
    components(schemas(
        CloudEvent,
        EventPublisherType,
        HttpPublisherConfig,
        NatsPublisherConfig,
        HttpEventPublisherType,
        NatsEventPublisherType,
        HttpEventPublisherConfigDto,
        NatsEventPublisherConfigDto,
        EventPublisherConfigDto,
        EventPublisherDto,
        EventPublisherListDto,
        CreateEventPublisherRequest,
        UpdateEventPublisherRequest,
        ActivateEventPublisherRequest,
        DeactivateEventPublisherRequest,
        DeleteEventPublisherRequest,
    )),
    tags(
        (name = "Events", description = "Stream domain events as CloudEvents via SSE and manage runtime event publishers.")
    )
)]
pub struct EventsApi;
