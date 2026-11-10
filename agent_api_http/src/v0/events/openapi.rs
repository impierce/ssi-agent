use crate::v0::events::{
    __path_events_sse_handler,
    commands::{
        activate_event_publisher::{__path_activate_event_publisher, ActivateEventPublisherRequest},
        create_event_publisher::{__path_create_event_publisher, CreateEventPublisherRequest},
        deactivate_event_publisher::{__path_deactivate_event_publisher, DeactivateEventPublisherRequest},
        delete_event_publisher::{__path_delete_event_publisher, DeleteEventPublisherRequest},
        update_event_publisher::{__path_update_event_publisher, UpdateEventPublisherRequest},
    },
    config::{
        EventPublisherConfigDto, HttpEventPublisherConfigDto, HttpEventPublisherType, NatsEventPublisherConfigDto,
        NatsEventPublisherType,
    },
    queries::{
        get_event_publisher_by_id::{__path_get_event_publisher_by_id, EventPublisherDto},
        get_event_publisher_list::{__path_get_event_publisher_list, EventPublisherListDto},
    },
};
use bc_event_publishing::domain::event_publisher::config::{EventPublisherFilter, EventPublisherType};
use shared_kernel::event_bus::CloudEvent;
use utoipa::{
    openapi::{
        path::{HttpMethod, OperationBuilder, PathItem, Paths, PathsBuilder},
        request_body::RequestBodyBuilder,
        ContentBuilder, Ref, Required, ResponseBuilder,
    },
    Modify, OpenApi,
};

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
        EventPublisherFilter,
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

/// The name of the webhook the HTTP event publisher calls.
pub const CLOUD_EVENT_WEBHOOK: &str = "cloud-event";

/// The requests this application sends on its own initiative, keyed by webhook name.
///
/// `OpenApi::merge` does not carry webhooks, so a document composed from this application's documents adds them
/// itself.
pub fn webhooks() -> Paths {
    PathsBuilder::new()
        .path(CLOUD_EVENT_WEBHOOK, cloud_event_webhook())
        .build()
}

/// The request sent to each active HTTP event publisher (`publisherType: "http"`).
fn cloud_event_webhook() -> PathItem {
    let operation = OperationBuilder::new()
        .summary(Some("Receive a CloudEvent"))
        .description(Some(
            "Sent to the `targetUrl` of every active HTTP event publisher (`publisherType: \"http\"`) whose \
            `filter.types` matches the event's `type`, with any custom `headers` configured on that publisher \
            (e.g. a shared secret).\n\n\
            Delivery is fire-and-forget: each request times out after 10 seconds and is not retried, and events are \
            dropped when the publisher falls behind the event bus. The response status is only logged.",
        ))
        .tag("Events")
        .request_body(Some(
            RequestBodyBuilder::new()
                .description(Some("The event, in the CloudEvents JSON format."))
                .content(
                    "application/cloudevents+json",
                    ContentBuilder::new()
                        .schema(Some(Ref::from_schema_name("CloudEvent")))
                        .build(),
                )
                .required(Some(Required::True))
                .build(),
        ))
        .response(
            "2XX",
            ResponseBuilder::new()
                .description("The event was received. Other statuses are logged and the event is not resent.")
                .build(),
        )
        .build();

    PathItem::new(HttpMethod::Post, operation)
}

/// Adds the [`webhooks`] to a document.
pub struct Webhooks;

impl Modify for Webhooks {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let webhooks = openapi.webhooks.get_or_insert_with(Paths::new);
        webhooks.paths.extend(self::webhooks().paths);
    }
}
