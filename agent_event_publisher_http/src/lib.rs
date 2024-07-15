use agent_issuance::{
    credential::aggregate::Credential, offer::aggregate::Offer, server_config::aggregate::ServerConfig,
};
use agent_shared::config::config;
use agent_store::{
    AuthorizationRequestEventPublisher, ConnectionEventPublisher, CredentialEventPublisher, EventPublisher,
    OfferEventPublisher, ServerConfigEventPublisher,
};
use agent_verification::{authorization_request::aggregate::AuthorizationRequest, connection::aggregate::Connection};
use async_trait::async_trait;
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, Query};
use serde::Deserialize;
use serde_with::skip_serializing_none;
use tracing::info;

#[cfg(feature = "test_utils")]
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
        // #[cfg(feature = "test_utils")]
        // let mut config = TEST_EVENT_PUBLISHER_HTTP_CONFIG
        //     .lock()
        //     .unwrap()
        //     .as_ref()
        //     .unwrap()
        //     .clone();
        // #[cfg(not(feature = "test_utils"))]
        // let mut config: serde_yaml::Value = {
        //     match std::fs::File::open("agent_event_publisher_http/config.yml") {
        //         Ok(config_file) => serde_yaml::from_reader(config_file)?,
        //         // If the config file does not exist, return an empty config.
        //         Err(_) => serde_yaml::Value::Null,
        //     }
        // };
        // let config = agent_shared::config::config("AGENT_EVENT_PUBLISHER_HTTP");

        let event_publisher_http = config().event_publishers.unwrap().http.unwrap();

        // let event_publishers = config.get_table("event_publishers").unwrap_or_default();
        // let event_publisher_http = event_publishers
        //     .get("http")
        //     .unwrap()
        //     .clone()
        //     .into_table()
        //     .unwrap_or_default();

        info!("event_publisher_http: {:?}", event_publisher_http);

        if event_publisher_http.enabled {
            // event_publisher_http.publishers
        }

        // TODO: map events to aggregates
        // let mapping = HashMap::<Event, DomainAggregate>::new();
        // let mapping: HashMap<Event, DomainAggregate> = HashMap::from([
        //     // credential
        //     (Event::UnsignedCredentialCreated, DomainAggregate::Credential),
        //     (Event::SignedCredentialCreated, DomainAggregate::Credential),
        //     (Event::CredentialSigned, DomainAggregate::Credential),
        //     // offer
        //     (Event::CredentialOfferCreated, DomainAggregate::Offer),
        //     // connection
        //     (Event::SIOPv2AuthorizationResponseVerified, DomainAggregate::Connection),
        // ]);

        let credential_events: Vec<String> = event_publisher_http
            .events
            .credential
            .unwrap_or(vec![])
            .iter()
            .map(|e| e.to_string())
            .collect();
        info!("credential_events: {:?}", credential_events);

        let event_publisher: EventPublisherHttp = EventPublisherHttp {
            server_config: None,
            credential: Some(AggregateEventPublisherHttp::new(
                event_publisher_http.target_url.clone(),
                credential_events,
            )),
            offer: None,
            connection: None,
            authorization_request: None,
        };

        info!("Loaded HTTP event publisher: {:?}", event_publisher);

        Ok(event_publisher)

        // let config = if event_publisher_http
        //     .get("enabled")
        //     .unwrap()
        //     .clone()
        //     .into_bool()
        //     .unwrap_or_default()
        // {
        //     let publishers: serde_yaml::Value = event_publisher_http
        //         .get("publishers")
        //         .unwrap()
        //         .clone()
        //         .try_deserialize()
        //         .unwrap();
        //     publishers
        // } else {
        //     serde_yaml::Value::Null
        // };

        // serde_yaml::from_value(config)
        //     .map_err(Into::into)
        //     .inspect(|event_publisher| {
        //         info!("Loaded HTTP event publisher: {:?}", event_publisher);
        //     })
    }
}

impl EventPublisher for EventPublisherHttp {
    fn server_config(&mut self) -> Option<ServerConfigEventPublisher> {
        self.server_config
            .take()
            .map(|publisher| Box::new(publisher) as ServerConfigEventPublisher)
    }

    fn credential(&mut self) -> Option<CredentialEventPublisher> {
        self.credential
            .take()
            .map(|publisher| Box::new(publisher) as CredentialEventPublisher)
    }

    fn offer(&mut self) -> Option<OfferEventPublisher> {
        self.offer
            .take()
            .map(|publisher| Box::new(publisher) as OfferEventPublisher)
    }

    fn connection(&mut self) -> Option<ConnectionEventPublisher> {
        self.connection
            .take()
            .map(|publisher| Box::new(publisher) as ConnectionEventPublisher)
    }

    fn authorization_request(&mut self) -> Option<AuthorizationRequestEventPublisher> {
        self.authorization_request
            .take()
            .map(|publisher| Box::new(publisher) as AuthorizationRequestEventPublisher)
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
                let request = self.client.post(&self.target_url).json(&event.payload);

                info!(
                    "Dispatching event: {:?} to HTTP endpoint: {:?}",
                    &event.payload, &self.target_url
                );

                // Send the request in a separate thread so that we don't have to await the response in the current thread.
                tokio::task::spawn(async move {
                    request
                        .send()
                        .await
                        .inspect(|response| {
                            info!("Response: {:?}", response);
                        })
                        .inspect_err(|error| {
                            info!("Error: {:?}", error);
                        })
                        .ok();
                });
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

    #[tokio::test(flavor = "multi_thread")]
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
            offer_id: Default::default(),
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

        // Wait for the request to arrive at the mock server endpoint.
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Assert that the event was dispatched to the target URL.
        assert_eq!(
            offer_event,
            serde_json::from_slice(&mock_server.received_requests().await.unwrap().first().unwrap().body).unwrap()
        );

        // A new event for the `Offer` aggregate that the publisher is not interested in.
        let offer_event = OfferEvent::CredentialsAdded {
            offer_id: Default::default(),
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
