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

// This struct holds all the different aggregate event publishers. For now it only contains Offer,
// but in the future it could house others like Credential, Identity, etc.
#[derive(Default, Debug)]
pub struct EventPublisherNats {
    pub offer: Option<AggregateEventPublisherNats<Offer>>,
}

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
        // Loads NATS configuration from the config file.
        let event_publisher_nats = config().event_publishers.nats.clone().unwrap_or_default();

        // If NATS is not enabled, return an empty event publisher.
        if !event_publisher_nats.enabled {
            return Ok(EventPublisherNats::default());
        }

        let offer = if !event_publisher_nats.events.offer.is_empty() {
            Some(
                // Call the new() constructor to populate the struct.
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
        // Generate unique id for each CloudEvent
        let event_id = format!("{}-{}", offer_id, Uuid::new_v4());

        // Construct the CloudEvent
        let event = EventBuilderV10::new()
            .id(event_id)
            .source("https://issuer.impierce.com/oid4vci/issuance-service")
            .ty("email.command.txcode.generated")
            .data(
                "application/json",
                json!({
                    "SendEmail": {
                        "recipient_email": recipient_email,
                        "template": "transaction_code",
                        "values": {
                            "transaction_code": tx_code
                        }
                    }
                }),
            )
            .build()?;

        // Convert Cloudevent into a suitable NATS message format
        let nats_event = NatsCloudEvent::from_event(event)?;

        println!(
            "Publishing to NATS subject '{}': {}",
            self.subject,
            String::from_utf8_lossy(&nats_event.payload)
        );

        let payload = nats_event.payload.into();
        let subject = self.subject.clone();

        self.client.publish(subject, payload).await?;
        info!("Published transaction code to NATS subject: {}", self.subject);

        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_generate_event_id() {
        let offer_id = "offer-123";
        let event_id = format!("{}-{}", offer_id, Uuid::new_v4());
        assert!(event_id.starts_with(offer_id));

        println!("Generated event_id: {}", event_id);
    }

    #[tokio::test]
    async fn test_nats_payload_format() {
        // Create a test CloudEvent
        let event = EventBuilderV10::new()
            .id("test-123")
            .source("https://impierce.com/offer")
            .ty("email.command.txcode.generated")
            .data(
                "application/json",
                json!({
                    "SendEmail": {
                        "recipient_email": "andres@rocarey.com",
                        "template": "transaction_code",
                        "values": {
                            "transaction_code": "997755"
                        }
                    }
                }),
            )
            .build()
            .unwrap();
        // Wrap the CloudEvent into a NATS message format
        let nats_event = NatsCloudEvent::from_event(event).unwrap();

        println!("NATS Payload:");
        println!("{}", String::from_utf8_lossy(&nats_event.payload));
    }

    #[tokio::test]
    async fn test_integration() {
        // Test creating publisher and publishing
        // For this to run successfully, you should have a NATS server running locally.
        // You can run one with Docker: `docker run -p 4222:4222 -ti nats:latest` in your terminal
        // before running this test.

        let publisher = AggregateEventPublisherNats::<Offer>::new(
            "nats://localhost:4222".to_string(),
            "test.commands".to_string(),
            vec!["TxCodeGenerated".to_string()],
        )
        .await;

        match publisher {
            Ok(p) => {
                println!("Connection to NATS successful");

                // Test publishing
                let result = p
                    .dispatch_tx_code_generated(
                        "offer-123".to_string(),
                        "12345".to_string(),
                        Some("sergey@kuryokhin.com".to_string()),
                    )
                    .await;

                match result {
                    Ok(_) => println!("Message published successfully and is now on its way to the client! "),
                    Err(e) => println!("Publishing failed: {}", e),
                }
            }
            Err(e) => println!("NATS connection failed: {}", e),
        }
    }
}
