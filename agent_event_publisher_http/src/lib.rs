use agent_authorization::domain::{
    access_token::aggregate::AccessToken, authorization_code::aggregate::AuthorizationCode, client::aggregate::Client,
    oauth2_authorization_request::aggregate::OAuth2AuthorizationRequest,
};
use agent_holder::presentation::aggregate::Presentation;
use agent_identity::{
    connection::aggregate::Connection, document::aggregate::Document, profile::aggregate::Profile,
    service::aggregate::Service,
};
use agent_issuance::{
    credential::aggregate::Credential,
    nonce::{self, aggregate::Nonce},
    offer::aggregate::Offer,
    server_config::aggregate::ServerConfig,
};
use agent_library::template::aggregate::Template;
use agent_shared::config::config;
use agent_store::{
    AccessTokenEventPublisher, AuthorizationCodeEventPublisher, AuthorizationRequestEventPublisher,
    ClientEventPublisher, ConnectionEventPublisher, CredentialEventPublisher, DocumentEventPublisher, EventPublisher,
    HolderCredentialEventPublisher, NonceEventPublisher, OAuth2AuthorizationRequestEventPublisher, OfferEventPublisher,
    PresentationEventPublisher, ProfileEventPublisher, ReceivedOfferEventPublisher, ServerConfigEventPublisher,
    ServiceEventPublisher, TemplateEventPublisher,
};
use agent_verification::authorization_request::aggregate::AuthorizationRequest;
use async_trait::async_trait;
use cqrs_es::{Aggregate, DomainEvent, EventEnvelope, Query};
use serde::Deserialize;
use serde_with::skip_serializing_none;
use tracing::info;

/// A struct that contains all the event publishers for the different aggregates.
#[skip_serializing_none]
#[derive(Debug, Deserialize, Default)]
pub struct EventPublisherHttp {
    // Authorization
    pub access_token: Option<AggregateEventPublisherHttp<AccessToken>>,
    pub authorization_code: Option<AggregateEventPublisherHttp<AuthorizationCode>>,
    pub client: Option<AggregateEventPublisherHttp<Client>>,
    pub oauth2_authorization_request: Option<AggregateEventPublisherHttp<OAuth2AuthorizationRequest>>,

    // Identity
    pub connection: Option<AggregateEventPublisherHttp<Connection>>,
    pub document: Option<AggregateEventPublisherHttp<Document>>,
    pub profile: Option<AggregateEventPublisherHttp<Profile>>,
    pub service: Option<AggregateEventPublisherHttp<Service>>,

    // Library
    pub template: Option<AggregateEventPublisherHttp<Template>>,

    // Issuance
    pub server_config: Option<AggregateEventPublisherHttp<ServerConfig>>,
    pub credential: Option<AggregateEventPublisherHttp<Credential>>,
    pub offer: Option<AggregateEventPublisherHttp<Offer>>,
    pub nonce: Option<AggregateEventPublisherHttp<Nonce>>,

    // Holder
    pub holder_credential: Option<AggregateEventPublisherHttp<agent_holder::credential::aggregate::Credential>>,
    pub presentation: Option<AggregateEventPublisherHttp<Presentation>>,
    pub received_offer: Option<AggregateEventPublisherHttp<agent_holder::offer::aggregate::Offer>>,

    // Verification
    pub authorization_request: Option<AggregateEventPublisherHttp<AuthorizationRequest>>,
}

impl EventPublisherHttp {
    pub fn load() -> anyhow::Result<Self> {
        let event_publisher_http = config().event_publishers.http.clone().unwrap_or_default();

        // If it's not enabled, return an empty event publisher.
        if !event_publisher_http.enabled {
            return Ok(EventPublisherHttp::default());
        }

        let access_token = (!event_publisher_http.events.access_token.is_empty()).then(|| {
            AggregateEventPublisherHttp::<AccessToken>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .access_token
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let authorization_code = (!event_publisher_http.events.authorization_code.is_empty()).then(|| {
            AggregateEventPublisherHttp::<AuthorizationCode>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .authorization_code
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let client = (!event_publisher_http.events.client.is_empty()).then(|| {
            AggregateEventPublisherHttp::<Client>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .client
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let oauth2_authorization_request =
            (!event_publisher_http.events.oauth2_authorization_request.is_empty()).then(|| {
                AggregateEventPublisherHttp::<OAuth2AuthorizationRequest>::new(
                    event_publisher_http.target_url.clone(),
                    event_publisher_http.headers.clone(),
                    event_publisher_http
                        .events
                        .oauth2_authorization_request
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                )
            });

        let connection = (!event_publisher_http.events.connection.is_empty()).then(|| {
            AggregateEventPublisherHttp::<Connection>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .connection
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let document = (!event_publisher_http.events.document.is_empty()).then(|| {
            AggregateEventPublisherHttp::<Document>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .document
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let profile = (!event_publisher_http.events.profile.is_empty()).then(|| {
            AggregateEventPublisherHttp::<Profile>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .profile
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let service = (!event_publisher_http.events.service.is_empty()).then(|| {
            AggregateEventPublisherHttp::<Service>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .service
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let template = (!event_publisher_http.events.template.is_empty()).then(|| {
            AggregateEventPublisherHttp::<Template>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .template
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let server_config = (!event_publisher_http.events.server_config.is_empty()).then(|| {
            AggregateEventPublisherHttp::<ServerConfig>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .server_config
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let credential = (!event_publisher_http.events.credential.is_empty()).then(|| {
            AggregateEventPublisherHttp::<Credential>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .credential
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let offer = (!event_publisher_http.events.offer.is_empty()).then(|| {
            AggregateEventPublisherHttp::<Offer>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .offer
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let nonce = (!event_publisher_http.events.nonce.is_empty()).then(|| {
            AggregateEventPublisherHttp::<Nonce>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .nonce
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let holder_credential = (!event_publisher_http.events.holder_credential.is_empty()).then(|| {
            AggregateEventPublisherHttp::<agent_holder::credential::aggregate::Credential>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .holder_credential
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let presentation = (!event_publisher_http.events.presentation.is_empty()).then(|| {
            AggregateEventPublisherHttp::<Presentation>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .presentation
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let received_offer = (!event_publisher_http.events.received_offer.is_empty()).then(|| {
            AggregateEventPublisherHttp::<agent_holder::offer::aggregate::Offer>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .received_offer
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let authorization_request = (!event_publisher_http.events.authorization_request.is_empty()).then(|| {
            AggregateEventPublisherHttp::<AuthorizationRequest>::new(
                event_publisher_http.target_url.clone(),
                event_publisher_http.headers.clone(),
                event_publisher_http
                    .events
                    .authorization_request
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        });

        let event_publisher: EventPublisherHttp = EventPublisherHttp {
            access_token,
            authorization_code,
            client,
            oauth2_authorization_request,
            connection,
            document,
            profile,
            service,
            template,
            server_config,
            credential,
            offer,
            nonce,
            holder_credential,
            presentation,
            received_offer,
            authorization_request,
        };

        info!("Loaded HTTP event publisher: {:?}", event_publisher);

        Ok(event_publisher)
    }
}

impl EventPublisher for EventPublisherHttp {
    fn connection(&mut self) -> Option<ConnectionEventPublisher> {
        self.connection
            .take()
            .map(|publisher| Box::new(publisher) as ConnectionEventPublisher)
    }

    fn document(&mut self) -> Option<DocumentEventPublisher> {
        self.document
            .take()
            .map(|publisher| Box::new(publisher) as DocumentEventPublisher)
    }

    fn profile(&mut self) -> Option<ProfileEventPublisher> {
        self.profile
            .take()
            .map(|publisher| Box::new(publisher) as ProfileEventPublisher)
    }

    fn service(&mut self) -> Option<ServiceEventPublisher> {
        self.service
            .take()
            .map(|publisher| Box::new(publisher) as ServiceEventPublisher)
    }

    fn template(&mut self) -> Option<TemplateEventPublisher> {
        self.template
            .take()
            .map(|publisher| Box::new(publisher) as TemplateEventPublisher)
    }

    fn authorization_code(&mut self) -> Option<AuthorizationCodeEventPublisher> {
        self.authorization_code
            .take()
            .map(|publisher| Box::new(publisher) as AuthorizationCodeEventPublisher)
    }
    fn client(&mut self) -> Option<ClientEventPublisher> {
        self.client
            .take()
            .map(|publisher| Box::new(publisher) as ClientEventPublisher)
    }
    fn oauth2_authorization_request(&mut self) -> Option<OAuth2AuthorizationRequestEventPublisher> {
        self.oauth2_authorization_request
            .take()
            .map(|publisher| Box::new(publisher) as OAuth2AuthorizationRequestEventPublisher)
    }
    fn access_token(&mut self) -> Option<AccessTokenEventPublisher> {
        self.access_token
            .take()
            .map(|publisher| Box::new(publisher) as AccessTokenEventPublisher)
    }

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

    fn nonce(&mut self) -> Option<NonceEventPublisher> {
        self.nonce
            .take()
            .map(|publisher| Box::new(publisher) as NonceEventPublisher)
    }

    fn holder_credential(&mut self) -> Option<HolderCredentialEventPublisher> {
        self.holder_credential
            .take()
            .map(|publisher| Box::new(publisher) as HolderCredentialEventPublisher)
    }

    fn presentation(&mut self) -> Option<PresentationEventPublisher> {
        self.presentation
            .take()
            .map(|publisher| Box::new(publisher) as PresentationEventPublisher)
    }

    fn received_offer(&mut self) -> Option<ReceivedOfferEventPublisher> {
        self.received_offer
            .take()
            .map(|publisher| Box::new(publisher) as ReceivedOfferEventPublisher)
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
    #[serde(with = "http_serde::option::header_map", default)]
    pub headers: Option<reqwest::header::HeaderMap>,
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
    pub fn new(target_url: String, headers: Option<reqwest::header::HeaderMap>, target_events: Vec<String>) -> Self {
        AggregateEventPublisherHttp {
            target_url,
            headers,
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
                let mut request = self.client.post(&self.target_url).json(&event.payload);

                if let Some(headers) = &self.headers {
                    request = request.headers(headers.clone());
                }

                info!(
                    "Dispatching event: {:?} to HTTP endpoint: {:?} with headers: {:?}",
                    event.payload, self.target_url, self.headers
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

    use agent_issuance::offer::aggregate::Status;
    use agent_issuance::offer::event::OfferEvent;
    use agent_shared::config::{set_config, Events};
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
        set_config().enable_event_publisher_http();
        set_config().set_event_publisher_http_target_url(target_url.clone());
        set_config().set_event_publisher_http_target_events(Events {
            offer: vec![agent_shared::config::OfferEvent::FormUrlEncodedCredentialOfferCreated],
            ..Default::default()
        });

        let publisher = EventPublisherHttp::load().unwrap();

        // A new event for the `Offer` aggregate.
        let offer_event = OfferEvent::FormUrlEncodedCredentialOfferCreated {
            offer_id: Default::default(),
            form_url_encoded_credential_offer: "form_url_encoded_credential_offer".to_string(),
            status: Status::Pending,
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

        let received_requests = mock_server.received_requests().await;
        let received_request = received_requests.as_ref().unwrap().first().unwrap();

        // Assert that the event was dispatched to the target URL.
        assert_eq!(offer_event, serde_json::from_slice(&received_request.body).unwrap());

        // Assert that the request contained the expected headers.
        assert_eq!(
            "Basic YWxhZGRpbjpvcGVuc2VzYW1l",
            received_request.headers.get("Authorization").unwrap()
        );

        // A new event for the `Offer` aggregate that the publisher is not interested in.
        let offer_event = OfferEvent::CredentialRequestVerified {
            offer_id: Default::default(),
            subject_id: Some("subject_id".to_string()),
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
