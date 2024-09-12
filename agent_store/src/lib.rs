use agent_issuance::{
    credential::aggregate::Credential, offer::aggregate::Offer, server_config::aggregate::ServerConfig,
};
use agent_verification::{authorization_request::aggregate::AuthorizationRequest, connection::aggregate::Connection};
use cqrs_es::Query;

pub mod in_memory;
pub mod postgres;

pub type ServerConfigEventPublisher = Box<dyn Query<ServerConfig>>;
pub type CredentialEventPublisher = Box<dyn Query<Credential>>;
pub type OfferEventPublisher = Box<dyn Query<Offer>>;
pub type HolderCredentialEventPublisher = Box<dyn Query<agent_holder::credential::aggregate::Credential>>;
pub type ReceivedOfferEventPublisher = Box<dyn Query<agent_holder::offer::aggregate::Offer>>;
pub type AuthorizationRequestEventPublisher = Box<dyn Query<AuthorizationRequest>>;
pub type ConnectionEventPublisher = Box<dyn Query<Connection>>;

/// Contains all the event_publishers for each aggregate.
pub type Partitions = (
    Vec<ServerConfigEventPublisher>,
    Vec<CredentialEventPublisher>,
    Vec<OfferEventPublisher>,
    Vec<HolderCredentialEventPublisher>,
    Vec<ReceivedOfferEventPublisher>,
    Vec<AuthorizationRequestEventPublisher>,
    Vec<ConnectionEventPublisher>,
);

/// An outbound event_publisher is a component that listens to events and dispatches them to the appropriate service. For each
/// aggregate, by default, `None` is returned. If an event_publisher is interested in a specific aggregate, it should return a
/// `Some` with the appropriate query.
// TODO: move this to a separate crate that will include all the logic for event_publishers, i.e. `agent_event_publisher`.
pub trait EventPublisher {
    fn server_config(&mut self) -> Option<ServerConfigEventPublisher> {
        None
    }
    fn credential(&mut self) -> Option<CredentialEventPublisher> {
        None
    }
    fn offer(&mut self) -> Option<OfferEventPublisher> {
        None
    }

    fn holder_credential(&mut self) -> Option<HolderCredentialEventPublisher> {
        None
    }
    fn received_offer(&mut self) -> Option<ReceivedOfferEventPublisher> {
        None
    }

    fn connection(&mut self) -> Option<ConnectionEventPublisher> {
        None
    }
    fn authorization_request(&mut self) -> Option<AuthorizationRequestEventPublisher> {
        None
    }
}

pub(crate) fn partition_event_publishers(event_publishers: Vec<Box<dyn EventPublisher>>) -> Partitions {
    event_publishers.into_iter().fold(
        (vec![], vec![], vec![], vec![], vec![], vec![], vec![]),
        |mut partitions, mut event_publisher| {
            if let Some(server_config) = event_publisher.server_config() {
                partitions.0.push(server_config);
            }
            if let Some(credential) = event_publisher.credential() {
                partitions.1.push(credential);
            }
            if let Some(offer) = event_publisher.offer() {
                partitions.2.push(offer);
            }

            if let Some(credential) = event_publisher.holder_credential() {
                partitions.3.push(credential);
            }
            if let Some(offer) = event_publisher.received_offer() {
                partitions.4.push(offer);
            }

            if let Some(authorization_request) = event_publisher.authorization_request() {
                partitions.5.push(authorization_request);
            }
            if let Some(connection) = event_publisher.connection() {
                partitions.6.push(connection);
            }
            partitions
        },
    )
}

#[cfg(test)]
mod test {
    use async_trait::async_trait;
    use cqrs_es::EventEnvelope;

    use super::*;

    struct TestServerConfigEventPublisher;

    #[async_trait]
    impl Query<ServerConfig> for TestServerConfigEventPublisher {
        async fn dispatch(&self, _aggregate_id: &str, _events: &[EventEnvelope<ServerConfig>]) {
            // Do something
        }
    }

    struct TestConnectionEventPublisher;

    #[async_trait]
    impl Query<Connection> for TestConnectionEventPublisher {
        async fn dispatch(&self, _aggregate_id: &str, _events: &[EventEnvelope<Connection>]) {
            // Do something
        }
    }

    struct FooEventPublisher;

    // This event_publisher is interested in both server_config and connections.
    impl EventPublisher for FooEventPublisher {
        fn server_config(&mut self) -> Option<ServerConfigEventPublisher> {
            Some(Box::new(TestServerConfigEventPublisher))
        }

        fn connection(&mut self) -> Option<ConnectionEventPublisher> {
            Some(Box::new(TestConnectionEventPublisher))
        }
    }

    struct BarEventPublisher;

    // This event_publisher is only interested in connections.
    impl EventPublisher for BarEventPublisher {
        fn connection(&mut self) -> Option<ConnectionEventPublisher> {
            Some(Box::new(TestConnectionEventPublisher))
        }
    }

    #[test]
    fn test_partition_event_publishers() {
        let event_publishers: Vec<Box<dyn EventPublisher>> =
            vec![Box::new(FooEventPublisher), Box::new(BarEventPublisher)];

        let (
            server_config_event_publishers,
            credential_event_publishers,
            offer_event_publishers,
            holder_credential_event_publishers,
            received_offer_event_publishers,
            authorization_request_event_publishers,
            connection_event_publishers,
        ) = partition_event_publishers(event_publishers);

        assert_eq!(server_config_event_publishers.len(), 1);
        assert_eq!(credential_event_publishers.len(), 0);
        assert_eq!(offer_event_publishers.len(), 0);
        assert_eq!(holder_credential_event_publishers.len(), 0);
        assert_eq!(received_offer_event_publishers.len(), 0);
        assert_eq!(authorization_request_event_publishers.len(), 0);
        assert_eq!(connection_event_publishers.len(), 2);
    }
}
