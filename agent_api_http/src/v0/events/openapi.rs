use crate::v0::events::__path_events_sse_handler;
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
    paths(events_sse_handler),
    components(schemas(CloudEvent)),
    tags(
        (name = "Events", description = "Stream domain events as CloudEvents via SSE.")
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

/// The request `agent_event_publisher_http` sends to each target configured under `event_publishers.http`.
fn cloud_event_webhook() -> PathItem {
    let operation = OperationBuilder::new()
        .summary(Some("Receive a CloudEvent"))
        .description(Some(
            "Sent to the `target_url` of every target configured under `event_publishers.http` whose `events.types` \
            include the event's type, with the headers configured for that target, e.g. a shared secret.\n\n\
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
