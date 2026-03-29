use agent_issuance::offer::aggregate::Offer;
use agent_shared::config::config;
use agent_store::{
    AccessTokenEventPublisher, AuthorizationCodeEventPublisher, AuthorizationRequestEventPublisher,
    ClientEventPublisher, ConnectionEventPublisher, CredentialEventPublisher, DataAccessConsentTokenEventPublisher,
    DocumentEventPublisher, EventPublisher, HolderCredentialEventPublisher, NonceEventPublisher,
    OAuth2AuthorizationRequestEventPublisher, OfferEventPublisher, PresentationEventPublisher, ProfileEventPublisher,
    ReceivedOfferEventPublisher, ServerConfigEventPublisher, ServiceEventPublisher, StatusListEventPublisher,
    TemplateEventPublisher,
};
use async_nats::Client;
use async_trait::async_trait;
use cloudevents::binding::nats::NatsCloudEvent;
use cloudevents::{EventBuilder, EventBuilderV10};
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, Query};
use std::error::Error;
use tracing::info;
use uuid::Uuid;

// This struct holds all the different aggregate event publishers. For now it only handles the Offer aggregate.
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
        // Get the NATS configuration. it's an Option<EventPublisherNats>
        let nats_config = match &config().event_publishers.nats {
            Some(config) => config.clone(),
            // Return default if no NATS configuration is provided.
            None => return Ok(EventPublisherNats::default()),
        };

        // If nats is not enabled, return an empty event publisher.
        if !nats_config.enabled {
            return Ok(EventPublisherNats::default());
        }

        let mut offer = None;

        // Iterate through the list of subjects from the config.
        for subject_config in &nats_config.subjects {
            if !subject_config.events.offer.is_empty() {
                offer = Some(
                    AggregateEventPublisherNats::<Offer>::new(
                        nats_config.nats_url.clone(),
                        subject_config.name.clone(),
                        subject_config.events.offer.iter().map(ToString::to_string).collect(),
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to create NATS client: {}", e))?,
                );
                break;
                // TODO: Extend this to loop for other aggregates if added.
            }
        }

        let event_publisher = EventPublisherNats { offer };

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

    fn nonce(&mut self) -> Option<NonceEventPublisher> {
        None
    }

    fn status_list(&mut self) -> Option<StatusListEventPublisher> {
        None
    }

    fn document(&mut self) -> Option<DocumentEventPublisher> {
        None
    }

    fn profile(&mut self) -> Option<ProfileEventPublisher> {
        None
    }

    fn service(&mut self) -> Option<ServiceEventPublisher> {
        None
    }

    fn template(&mut self) -> Option<TemplateEventPublisher> {
        None
    }

    fn authorization_code(&mut self) -> Option<AuthorizationCodeEventPublisher> {
        None
    }

    fn client(&mut self) -> Option<ClientEventPublisher> {
        None
    }

    fn oauth2_authorization_request(&mut self) -> Option<OAuth2AuthorizationRequestEventPublisher> {
        None
    }

    fn access_token(&mut self) -> Option<AccessTokenEventPublisher> {
        None
    }

    fn server_config(&mut self) -> Option<ServerConfigEventPublisher> {
        None
    }

    fn connection(&mut self) -> Option<ConnectionEventPublisher> {
        None
    }

    fn credential(&mut self) -> Option<CredentialEventPublisher> {
        None
    }

    fn holder_credential(&mut self) -> Option<HolderCredentialEventPublisher> {
        None
    }

    fn presentation(&mut self) -> Option<PresentationEventPublisher> {
        None
    }

    fn received_offer(&mut self) -> Option<ReceivedOfferEventPublisher> {
        None
    }

    fn authorization_request(&mut self) -> Option<AuthorizationRequestEventPublisher> {
        None
    }

    fn data_access_consent_token(&mut self) -> Option<DataAccessConsentTokenEventPublisher> {
        None
    }
}

#[async_trait]
impl<A> Query<A> for AggregateEventPublisherNats<A>
where
    A: Aggregate,
    A::Event: serde::Serialize + DomainEvent,
{
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<A>]) {
        for event in events {
            if self.target_events.contains(&event.payload.event_type()) {
                if let Err(e) = self.dispatch_event(aggregate_id, &event.payload).await {
                    tracing::error!(
                        "Failed to dispatch {} event for aggregate {}: {}",
                        event.payload.event_type(),
                        aggregate_id,
                        e
                    );
                }
            }
        }
    }
}

impl<A> AggregateEventPublisherNats<A>
where
    A: Aggregate,
    A::Event: serde::Serialize,
{
    async fn dispatch_event(&self, aggregate_id: &str, event: &A::Event) -> Result<(), Box<dyn Error>>
    where
        A::Event: DomainEvent,
    {
        // Generate a unique Id for each CloudEvent
        let event_id = format!("{}-{}", aggregate_id, Uuid::new_v4());
        let event_type = event.event_type();

        // Use application URL from the configuration as the event source
        let event_source = config().application_url.to_string();

        // Construct the CloudEvent
        let cloud_event = EventBuilderV10::new()
            .id(event_id)
            .source(event_source)
            .ty(format!("offer.event.{}", event_type.to_lowercase()))
            .data("application/json", serde_json::to_value(event)?)
            .build()?;

        // Convert Cloudevent into a formatted NATS message
        let nats_event = NatsCloudEvent::from_event(cloud_event)?;

        info!(
            "Publishing {} for aggregate_id {} to NATS subject '{}': {}",
            event_type,
            aggregate_id,
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
    use agent_issuance::offer::aggregate::DeliveryOptions;
    use agent_issuance::offer::event::OfferEvent;
    use serde_json::json;

    #[test]
    fn generate_test_event_id() {
        let aggregate_id = "aggregate-12345";
        let event_id = format!("{}-{}", aggregate_id, Uuid::new_v4());
        println!("Event-ID: {}", event_id);
    }

    #[tokio::test]
    async fn generate_test_nats_payload() {
        // Create a test CloudEvent
        let event = EventBuilderV10::new()
            .id("test-123")
            .source("https://impierce.com/offer")
            .ty("email.command.txcodegenerated")
            .data(
                "application/json",
                json!({
                    "TxCodeGenerated": {
                        "offer_id": "12345",
                        "tx_code": "1234",
                        "delivery_options": {
                            "recipient_email": "andres-rocarey@example.test"
                        },
                    }
                }),
            )
            .build()
            .unwrap();

        // Wrap the CloudEvent into a NATS message format
        let nats_event = NatsCloudEvent::from_event(event).unwrap();
        let payload_str = String::from_utf8_lossy(&nats_event.payload);

        assert!(!payload_str.is_empty());
        assert!(payload_str.contains("TxCodeGenerated"));
        assert!(payload_str.contains("1234"));

        println!("NATS Payload:");
        println!("{}", payload_str);
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
                info!("Connection to NATS successful");

                // Test publishing
                let test_event = OfferEvent::TxCodeGenerated {
                    offer_id: "offer-123".to_string(),
                    tx_code: "12345".to_string(),
                    delivery_options: Some(DeliveryOptions {
                        recipient_email: Some("sergey.kuryokhin@example.test".to_string()),
                    }),
                };

                let result = p.dispatch_event("offer-123", &test_event).await;

                match result {
                    Ok(_) => info!("Message published successfully and is now on its way to the client! "),
                    Err(e) => info!("Publishing failed: {}", e),
                }
            }
            Err(e) => info!("NATS connection failed: {}", e),
        }
    }
}
