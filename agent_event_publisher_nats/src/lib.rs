use agent_issuance::{offer::event::OfferEvent, Offer};
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, Query};
use cloudevents::{EventBuilder, EventBuilderV10};
use serde_json::json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// This can be populated for each aggregate type, e.g. Credential, Received_Offer, etc.
#[skip_serializing_none]
#[derive(Default, Debug, Deserialize)]
pub struct EventPublisherNats {
    pub offer: Option<AggregateEventPublisherNats<Offer>>,
}

/// This contains infrastructure data from the 
#[skip_serializing_none]
#[derive(Debug, Deserialize)]
        pub struct AggregateEventPublisherNats<A>
        where
            A: Aggregate,
        {
            pub nats_client: async_nats::Client,
            pub subject: String, 
            pub target_events: Vec<String>,
        }

        impl<A> AggregateEventPublisherNats<A>
        where
            A: Aggregate,
        {
            pub fn new(nats_client: async_nats::Client, subject: String, target_events: Vec<String>) -> Self {
                AggregateEventPublisherNats {
                    nats_client,
                    subject,
                    target_events,
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

    // This can be populated for each aggregate type. 
    impl EventPublisher for EventPublisherNats {
        fn offer(&mut self) -> Option<OfferEventPublisher> {
            self.offer
                .take() 
                .map(|publisher| Box::new(publisher) as OfferEventPublisher)
        }
    }

    impl<A> Query<A> for AggregateEventPublisherNats<A>
    where
        A: Aggregate,
        { 
            async fn dispatch(&self, events: &[EventEnvelope<A>]) -> Result<(), Error> {
                for event in events {
                    if self.target_events.contains(&event.payload.event_type()) {
                        match event.payload { 
                            OfferEvent::TxCodeGenerated { offer_id, tx_code} => {
                                self.dispatch_tx_code_generated(offer_id, tx_code).await?;
                            }
                            _ => { return Ok(()); } // Ignore other events for now
                        }
                    }
                }
                Ok(())
            }
        }

        pub fn get_recipient_from_offer(event) -> String {
            todo!()
            // there needs to be some kind of matching or system which links the emails to the offer. 
        }
            async fn dispatch(&self, events: &[EventEnvelope<A>]) -> Result<(), Error> {
                for event in events {
                    if self.target_events.contains(&event.payload.event_type()) {
                        match event.payload { 
                            OfferEvent::TxCodeGenerated { offer_id, tx_code} => {
                                self.dispatch_tx_code_generated(offer_id, tx_code).await?;
                            }
                            _ => { return ... not sure yet. }
                        }
                    }
                }

                async fn dispatch_tx_code_generated(offer_id, tx_code) -> Result<(), Error> {
                    let recipient = get_recipient_from_offer(offer_id);
                    self.nats_client.publish(&self.subject, message).await?;
                    Ok(())
                }


                