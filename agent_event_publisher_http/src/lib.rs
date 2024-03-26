use agent_issuance::{
    credential::aggregate::Credential, offer::aggregate::Offer, server_config::aggregate::ServerConfig,
};
use agent_store::OutboundAdapter;
use agent_verification::{authorization_request::aggregate::AuthorizationRequest, connection::aggregate::Connection};
use async_trait::async_trait;
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, Query};
use serde::Deserialize;
use serde_with::skip_serializing_none;

#[cfg(feature = "test")]
pub static TEST_EVENT_PUBLISHER_HTTP_CONFIG: std::sync::Mutex<Option<serde_yaml::Value>> = std::sync::Mutex::new(None);

/// A struct that contains all the event publishers for the different aggregates.
#[skip_serializing_none]
#[derive(Debug, Deserialize)]
pub struct EventPublisherHttp {
    // Issuance
    pub server_config: Option<AggregateEventPublisherHttp<ServerConfig>>,
    pub credential: Option<AggregateEventPublisherHttp<Credential>>,
    pub offer: Option<AggregateEventPublisherHttp<Offer>>,

    // Verification
    pub connection: Option<AggregateEventPublisherHttp<Connection>>,
    pub authorization_request: Option<AggregateEventPublisherHttp<AuthorizationRequest>>,
}

impl EventPublisherHttp {
    pub fn load() -> anyhow::Result<Self> {
        #[cfg(feature = "test")]
        let mut config = TEST_EVENT_PUBLISHER_HTTP_CONFIG
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .clone();
        #[cfg(not(feature = "test"))]
        let mut config: serde_yaml::Value = {
            match std::fs::File::open("agent_event_publisher_http/config.yml") {
                Ok(config_file) => serde_yaml::from_reader(config_file)?,
                // If the config file does not exist, return an empty config.
                Err(_) => serde_yaml::Value::Null,
            }
        };

        config.apply_merge()?;

        serde_yaml::from_value(config).map_err(Into::into)
    }
}

impl OutboundAdapter for EventPublisherHttp {
    fn server_config(&mut self) -> Option<Box<dyn Query<ServerConfig>>> {
        self.server_config
            .take()
            .map(|publisher| Box::new(publisher) as Box<dyn Query<ServerConfig>>)
    }

    fn credential(&mut self) -> Option<Box<dyn Query<Credential>>> {
        self.credential
            .take()
            .map(|publisher| Box::new(publisher) as Box<dyn Query<Credential>>)
    }

    fn offer(&mut self) -> Option<Box<dyn Query<Offer>>> {
        self.offer
            .take()
            .map(|publisher| Box::new(publisher) as Box<dyn Query<Offer>>)
    }

    fn connection(&mut self) -> Option<Box<dyn Query<Connection>>> {
        self.connection
            .take()
            .map(|publisher| Box::new(publisher) as Box<dyn Query<Connection>>)
    }

    fn authorization_request(&mut self) -> Option<Box<dyn Query<AuthorizationRequest>>> {
        self.authorization_request
            .take()
            .map(|publisher| Box::new(publisher) as Box<dyn Query<AuthorizationRequest>>)
    }
}

/// An event publisher for a specific aggregate that dispatches events to an HTTP endpoint.
#[skip_serializing_none]
#[derive(Debug, Deserialize)]
pub struct AggregateEventPublisherHttp<A>
where
    A: Aggregate,
{
    pub target_url: String,
    pub target_events: Vec<String>,
    #[serde(skip)]
    pub client: reqwest::Client,
    #[serde(skip)]
    _marker: std::marker::PhantomData<A>,
}

impl<A> AggregateEventPublisherHttp<A>
where
    A: Aggregate,
{
    pub fn new(target_url: String, target_events: Vec<String>) -> Self {
        AggregateEventPublisherHttp {
            target_url,
            target_events,
            client: reqwest::Client::new(),
            _marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<A> Query<A> for AggregateEventPublisherHttp<A>
where
    A: Aggregate,
{
    async fn dispatch(&self, _view_id: &str, events: &[EventEnvelope<A>]) {
        for event in events {
            if self.target_events.contains(&event.payload.event_type()) {
                self.client
                    .post(&self.target_url)
                    .json(&event.payload)
                    .send()
                    .await
                    .ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use agent_issuance::offer::event::OfferEvent;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn it_works() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/ssi-events-subscriber"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let target_url = format!("{}/ssi-events-subscriber", &mock_server.uri());

        // Set the test configuration.
        TEST_EVENT_PUBLISHER_HTTP_CONFIG.lock().unwrap().replace(
            serde_yaml::from_str(&format!(
                r#"
                    target_url: &target_url {target_url}

                    offer: {{
                        target_url: *target_url,
                        target_events: [
                            FormUrlEncodedCredentialOfferCreated
                        ]
                    }}
                "#
            ))
            .unwrap(),
        );

        let publisher = EventPublisherHttp::load().unwrap();

        // A new event for the `Offer` aggregate.
        let offer_event = OfferEvent::FormUrlEncodedCredentialOfferCreated {
            form_url_encoded_credential_offer: "form_url_encoded_credential_offer".to_string(),
        };

        let events = [EventEnvelope::<Offer> {
            aggregate_id: "offer-0001".to_string(),
            sequence: 0,
            payload: offer_event.clone(),
            metadata: Default::default(),
        }];

        // Dispatch the event.
        publisher.offer.as_ref().unwrap().dispatch("view_id", &events).await;

        // Assert that the event was dispatched to the target URL.
        assert_eq!(
            offer_event,
            serde_json::from_slice(&mock_server.received_requests().await.unwrap().first().unwrap().body).unwrap()
        );

        // A new event for the `Offer` aggregate that the publisher is not interested in.
        let offer_event = OfferEvent::CredentialsAdded {
            credential_ids: vec!["credential-0001".to_string()],
        };

        let events = [EventEnvelope::<Offer> {
            aggregate_id: "offer-0002".to_string(),
            sequence: 0,
            payload: offer_event.clone(),
            metadata: Default::default(),
        }];

        // Dispatch the event.
        publisher.offer.as_ref().unwrap().dispatch("view_id", &events).await;

        // Assert that the event was not dispatched to the target URL.
        assert!(mock_server.received_requests().await.unwrap().len() == 1);
    }
}
