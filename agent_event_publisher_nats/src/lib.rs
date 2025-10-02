use agent_issuance::{offer::aggregate::Offer, offer::event::OfferEvent};
use agent_shared::config::config;
use agent_store::{EventPublisher, OfferEventPublisher};
use async_nats::Client;
use async_trait::async_trait;
use cloudevents::binding::nats::NatsCloudEvent;
use cloudevents::{EventBuilder, EventBuilderV10};
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, Query};
use serde_json::json;
use std::error::Error;
use tracing::info;
use uuid::Uuid;

/// This can be populated for each aggregate type, e.g. Credential, Received_Offer, etc.
#[derive(Default, Debug)]
pub struct EventPublisherNats {
    pub offer: Option<AggregateEventPublisherNats<Offer>>,
}

/// This contains infrastructure data from the config file.
#[derive(Debug)]
pub struct AggregateEventPublisherNats<A>
where
    A: Aggregate,
{
    pub nats_url: String,
    pub subject: String,
    pub target_events: Vec<String>,
    pub client: Client,
    _marker: std::marker::PhantomData<A>,
}

impl<A> AggregateEventPublisherNats<A>
where
    A: Aggregate,
{
    pub async fn new(nats_url: String, subject: String, target_events: Vec<String>) -> Result<Self, Box<dyn Error>> {
        let client = async_nats::connect(&nats_url).await?;

        Ok(AggregateEventPublisherNats {
            nats_url,
            subject,
            target_events,
            client,
            _marker: std::marker::PhantomData,
        })
    }
}

impl EventPublisherNats {
    pub async fn load() -> anyhow::Result<Self> {
        // This loads configuration from the config file.
        let event_publisher_nats = config().event_publishers.nats.clone().unwrap_or_default();

        // If NATS is not enabled, return an empty event publisher.
        if !event_publisher_nats.enabled {
            return Ok(EventPublisherNats::default());
        }

        let offer = if !event_publisher_nats.events.offer.is_empty() {
            Some(
                // Calling our new() constructor to populate the struct.
                AggregateEventPublisherNats::<Offer>::new(
                    event_publisher_nats.nats_url.clone(),
                    event_publisher_nats.subject.clone(),
                    event_publisher_nats
                        .events
                        .offer
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                )
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create NATS client: {}", e))?,
            )
        } else {
            None
        };

        let event_publisher: EventPublisherNats = EventPublisherNats { offer };

        info!("Loaded NATS event publisher: {:?}", event_publisher);

        Ok(event_publisher)
    }
}

impl EventPublisher for EventPublisherNats {
    fn offer(&mut self) -> Option<OfferEventPublisher> {
        self.offer
            .take()
            .map(|publisher| Box::new(publisher) as OfferEventPublisher)
    }
}

#[async_trait]
impl Query<Offer> for AggregateEventPublisherNats<Offer> {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<Offer>]) {
        for event in events {
            if self.target_events.contains(&event.payload.event_type()) {
                if let OfferEvent::TxCodeGenerated {
                    offer_id,
                    tx_code,
                    recipient_email,
                } = &event.payload
                {
                    if let Err(e) = self
                        .dispatch_tx_code_generated(offer_id.clone(), tx_code.clone(), recipient_email.clone())
                        .await
                    {
                        tracing::error!("Failed to dispatch tx code event for aggregate {}: {}", aggregate_id, e);
                    }
                }
            }
        }
    }
}
impl AggregateEventPublisherNats<Offer> {
    async fn dispatch_tx_code_generated(
        &self,
        offer_id: String,
        tx_code: String,
        recipient_email: Option<String>,
    ) -> Result<(), Box<dyn Error>> {
        // Generate unique id for CloudEvent
        let event_id = format!("{}-{}", offer_id, Uuid::new_v4());

        // Construct the CloudEvent
        let event = EventBuilderV10::new()
            .id(event_id)
            .source("https://impierce.com/special-offer")
            .ty("email.command.txcode.generated")
            .data(
                "application/json",
                json!({
                    "recipient_email": recipient_email,
                    "template": "transaction_code",
                    "values": tx_code,
                }),
            )
            .build()?;

        // Convert Cloudevent to NATS bytes format then publish
        let nats_event = NatsCloudEvent::from_event(event)?;

        let payload = nats_event.payload.into();

        let subject = self.subject.clone();

        self.client.publish(subject, payload).await?;

        info!("Published tx code event to NATS subject: {}", self.subject);

        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_generate_event_id() {
        let offer_id = "hotcakes_123";
        let event_id = format!("{}-{}", offer_id, Uuid::new_v4());
        assert!(event_id.starts_with(offer_id));

        println!("Generated event_id: {}", event_id);
    }

    #[tokio::test]
    async fn test_nats_payload_format() {
        // Create a test CloudEvent
        let event = EventBuilderV10::new()
            .id("test-123")
            .source("https://impierce.com/special-offer")
            .ty("email.command.txcode.generated")
            .data(
                "application/json",
                json!({
                    "recipient_email": "andres@rocarey.com",
                    "template": "transaction_code",
                    "values": "997755",
                }),
            )
            .build()
            .unwrap();

        let nats_event = NatsCloudEvent::from_event(event).unwrap();

        println!("NATS Payload:");
        println!("{}", String::from_utf8_lossy(&nats_event.payload));
    }
}
