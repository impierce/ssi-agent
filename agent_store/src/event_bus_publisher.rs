use async_trait::async_trait;
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, Query};
use shared_kernel::event_bus::{build_cloud_event, EventBusHandle};

/// A `cqrs_es::Query` implementation that forwards committed aggregate events to the [`EventBusHandle`].
pub struct EventBusPublisher<A: Aggregate> {
    bus: EventBusHandle,
    _phantom: std::marker::PhantomData<A>,
}

impl<A: Aggregate> EventBusPublisher<A> {
    pub fn new(bus: EventBusHandle) -> Self {
        Self {
            bus,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<A> Query<A> for EventBusPublisher<A>
where
    A: Aggregate,
    A::Event: serde::Serialize + DomainEvent,
{
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<A>]) {
        for envelope in events {
            let payload = match serde_json::to_value(&envelope.payload) {
                Ok(val) => val,
                Err(err) => {
                    tracing::error!("Failed to serialize event payload for EventBus: {:?}", err);
                    continue;
                }
            };

            let occurred_at = envelope
                .metadata
                .get("occurred_at")
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc));

            let cloud_event = build_cloud_event(
                A::TYPE,
                aggregate_id,
                envelope.sequence,
                &envelope.payload.event_type(),
                payload,
                occurred_at,
            );

            self.bus.publish(cloud_event);
        }
    }
}
