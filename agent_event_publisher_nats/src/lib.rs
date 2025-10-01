use agent_issuance::{offer::event::OfferEvent, offer::aggregate::Offer};
use agent_shared::config::config;
use async_nats::Client;
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, Query};
use serde::{Deserialize};
use std::error::Error;
use cloudevents::{EventBuilder, EventBuilderV10};
use serde_json::json;
use tracing::info;
use futures::StreamExt;
use uuid::Uuid;
use agent_store::{OfferEventPublisher, EventPublisher};

/// This can be populated for each aggregate type, e.g. Credential, Received_Offer, etc.
#[derive(Default, Debug, Deserialize)]
pub struct EventPublisherNats {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer: Option<AggregateEventPublisherNats<Offer>>,
}

/// This contains infrastructure data from the config file.
#[derive(Debug, Deserialize)]
        pub struct AggregateEventPublisherNats<A>
        where
            A: Aggregate,
        {
            pub nats_url: String,
            pub subject: String, 
            pub target_events: Vec<String>,
            #[serde(skip)]
            _marker: std::marker::PhantomData<A>,
        }

        // TODO!! CHAYE FIGURE OUT THE CLIENT SITUATION. SHOULD 
        impl<A> AggregateEventPublisherNats<A>
        where
            A: Aggregate,
        {
            pub fn new(nats_url: String, subject: String, target_events: Vec<String>) -> Self {
                AggregateEventPublisherNats {
                    nats_url,
                    subject,
                    target_events,
                    _marker: std::marker::PhantomData,
                }
                }
        }

impl EventPublisherNats {
    pub fn load() -> anyhow::Result<Self> {
    // This loads configuration from the config file. 
        let event_publisher_nats = config().event_publishers.nats.clone().unwrap_or_default();

        // If NATS is not enabled, return an empty event publisher.
        if !event_publisher_nats.enabled {
            return Ok(EventPublisherNats::default());
        }

       let offer = (!event_publisher_nats.events.offer.is_empty()).then(|| {
        // Calling our new() constructor
            AggregateEventPublisherNats::<Offer>::new(
                event_publisher_nats.nats_client.clone(),
                event_publisher_nats.subject.clone(),
                event_publisher_nats
                    .events
                    .offer
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let event_publisher: EventPublisherNats = EventPublisherNats {
            offer,
        };

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

    impl<A> Query<A> for AggregateEventPublisherNats<A>
    // The Query allows us to listen for events from the event store. 
    where
        A: Aggregate,
        { 
        
            async fn dispatch(&self, events: &[EventEnvelope<A>]) -> Result<(), Error> {
                for event in events {
                    if self.target_events.contains(&event.payload.event_type()) {
                        match event.payload {
                            OfferEvent::TxCodeGenerated { offer_id, tx_code, recipient_email } => {
                                self.dispatch_tx_code_generated(offer_id.clone(), tx_code.clone(), recipient_email.clone()).await?;
                            }
                        }
                            _ => { return Ok(()); }
                            // For now, ignore other events
                        }
                    }
                }
            
            }
impl<A> AggregateEventPublisherNats<A>
where A: Aggregate,
{
                async fn dispatch_tx_code_generated(&self, offer_id, tx_code, recipient_email) -> Result<(), Error> {
                    let template = "transaction_code";
                    // Generate unique id for CloudEvent
                    let event_id = format!("{}-{}", offer_id, Uuid::new_v4());

          // Construct the CloudEvent 
                 let event = EventBuilderV10::new()
                .id(&event_id)
                .source("https://impierce.com/special-offer")
                .ty("email.command.txcode.generated")
                .specversion("1.0")
                .data("application/json", json!({
                    "recipient_email": recipient_email, 
                    "template": "transaction_code",
                    "values": tx_code,
                }))
                .build()?;

                    self.nats_client.publish(&self.subject, event).await?;
                    Ok(())
                }
            }
    


        #[cfg(test)]
        pub mod tests {
            use super::*; 
            fn test_generate_event_id() {
                let offer_id = "hotcakes_123";
                let event_id = format!("{}-{}", offer_id, Uuid::new_v4());
                assert!(event_id.starts_with(offer_id));

                println!("Generated event_id: {}", event_id);
            }
        }
    