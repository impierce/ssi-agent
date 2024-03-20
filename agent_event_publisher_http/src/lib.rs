use agent_shared::config;
use async_trait::async_trait;
use cqrs_es::{Aggregate, EventEnvelope, Query};

/// An event publisher that dispatches events to an HTTP endpoint.
pub struct EventPublisherHttp<A>
where
    A: Aggregate,
{
    pub target_url: String,
    pub client: reqwest::Client,
    _phantom: std::marker::PhantomData<A>,
}

impl<A> EventPublisherHttp<A>
where
    A: Aggregate,
{
    pub fn new() -> Self {
        let target_url = config!("target_url").unwrap();
        let client = reqwest::Client::new();

        EventPublisherHttp {
            target_url,
            client,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<A> Default for EventPublisherHttp<A>
where
    A: Aggregate,
{
    fn default() -> Self {
        EventPublisherHttp::new()
    }
}

#[async_trait]
impl<A> Query<A> for EventPublisherHttp<A>
where
    A: Aggregate,
{
    async fn dispatch(&self, _view_id: &str, events: &[EventEnvelope<A>]) {
        for event in events {
            self.client
                .post(self.target_url.as_str())
                .json(&event.payload)
                .send()
                .await
                .unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use agent_verification::connection::aggregate::Connection;
    use agent_verification::connection::event::ConnectionEvent;
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

        std::env::set_var("TEST_TARGET_URL", &target_url);

        let publisher = EventPublisherHttp::new();

        // A new event for the `Connection` aggregate.
        let connection_event = ConnectionEvent::SIOPv2AuthorizationResponseVerified {
            id_token: "id_token".to_string(),
        };

        let events = [EventEnvelope::<Connection> {
            aggregate_id: "connection-0001".to_string(),
            sequence: 0,
            payload: connection_event.clone(),
            metadata: Default::default(),
        }];

        // Dispatch the event.
        publisher.dispatch("view_id", &events).await;

        // Assert that the event was dispatched to the target URL.
        assert_eq!(
            connection_event,
            serde_json::from_slice(&mock_server.received_requests().await.unwrap().first().unwrap().body).unwrap()
        );
    }
}
