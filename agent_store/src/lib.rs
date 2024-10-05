use agent_identity::{connection::aggregate::Connection, document::aggregate::Document, service::aggregate::Service};
use agent_issuance::{
    credential::aggregate::Credential, offer::aggregate::Offer, server_config::aggregate::ServerConfig,
};
use agent_verification::{
    authorization_request::aggregate::AuthorizationRequest, connection::aggregate::Connection as Connection2,
};
use cqrs_es::Query;

pub mod in_memory;
pub mod postgres;

pub type ConnectionEventPublisher = Box<dyn Query<Connection>>;
pub type DocumentEventPublisher = Box<dyn Query<Document>>;
pub type ServiceEventPublisher = Box<dyn Query<Service>>;
pub type ServerConfigEventPublisher = Box<dyn Query<ServerConfig>>;
pub type CredentialEventPublisher = Box<dyn Query<Credential>>;
pub type OfferEventPublisher = Box<dyn Query<Offer>>;
pub type HolderCredentialEventPublisher = Box<dyn Query<agent_holder::credential::aggregate::Credential>>;
pub type PresentationEventPublisher = Box<dyn Query<agent_holder::presentation::aggregate::Presentation>>;
pub type ReceivedOfferEventPublisher = Box<dyn Query<agent_holder::offer::aggregate::Offer>>;
pub type AuthorizationRequestEventPublisher = Box<dyn Query<AuthorizationRequest>>;
pub type ConnectionEventPublisher2 = Box<dyn Query<Connection2>>;

/// Contains all the event_publishers for each aggregate.
#[derive(Default)]
pub struct Partitions {
    pub connection_event_publishers: Vec<ConnectionEventPublisher>,
    pub document_event_publishers: Vec<DocumentEventPublisher>,
    pub service_event_publishers: Vec<ServiceEventPublisher>,
    pub server_config_event_publishers: Vec<ServerConfigEventPublisher>,
    pub credential_event_publishers: Vec<CredentialEventPublisher>,
    pub offer_event_publishers: Vec<OfferEventPublisher>,
    pub holder_credential_event_publishers: Vec<HolderCredentialEventPublisher>,
    pub presentation_event_publishers: Vec<PresentationEventPublisher>,
    pub received_offer_event_publishers: Vec<ReceivedOfferEventPublisher>,
    pub authorization_request_event_publishers: Vec<AuthorizationRequestEventPublisher>,
    pub connection2_event_publishers: Vec<ConnectionEventPublisher2>,
}

/// An outbound event_publisher is a component that listens to events and dispatches them to the appropriate service. For each
/// aggregate, by default, `None` is returned. If an event_publisher is interested in a specific aggregate, it should return a
/// `Some` with the appropriate query.
// TODO: move this to a separate crate that will include all the logic for event_publishers, i.e. `agent_event_publisher`.
pub trait EventPublisher {
    fn document(&mut self) -> Option<DocumentEventPublisher> {
        None
    }
    fn service(&mut self) -> Option<ServiceEventPublisher> {
        None
    }

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
    fn presentation(&mut self) -> Option<PresentationEventPublisher> {
        None
    }
    fn received_offer(&mut self) -> Option<ReceivedOfferEventPublisher> {
        None
    }

    fn connection(&mut self) -> Option<ConnectionEventPublisher2> {
        None
    }
    fn authorization_request(&mut self) -> Option<AuthorizationRequestEventPublisher> {
        None
    }
}

pub(crate) fn partition_event_publishers(event_publishers: Vec<Box<dyn EventPublisher>>) -> Partitions {
    event_publishers
        .into_iter()
        .fold(Partitions::default(), |mut partitions, mut event_publisher| {
            if let Some(document) = event_publisher.document() {
                partitions.document_event_publishers.push(document);
            }
            if let Some(service) = event_publisher.service() {
                partitions.service_event_publishers.push(service);
            }

            if let Some(server_config) = event_publisher.server_config() {
                partitions.server_config_event_publishers.push(server_config);
            }
            if let Some(credential) = event_publisher.credential() {
                partitions.credential_event_publishers.push(credential);
            }
            if let Some(offer) = event_publisher.offer() {
                partitions.offer_event_publishers.push(offer);
            }

            if let Some(holder_credential) = event_publisher.holder_credential() {
                partitions.holder_credential_event_publishers.push(holder_credential);
            }
            if let Some(presentation) = event_publisher.presentation() {
                partitions.presentation_event_publishers.push(presentation);
            }
            if let Some(received_offer) = event_publisher.received_offer() {
                partitions.received_offer_event_publishers.push(received_offer);
            }

            if let Some(authorization_request) = event_publisher.authorization_request() {
                partitions
                    .authorization_request_event_publishers
                    .push(authorization_request);
            }
            if let Some(connection) = event_publisher.connection() {
                partitions.connection2_event_publishers.push(connection);
            }
            partitions
        })
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

    struct TestConnectionEventPublisher2;

    #[async_trait]
    impl Query<Connection2> for TestConnectionEventPublisher2 {
        async fn dispatch(&self, _aggregate_id: &str, _events: &[EventEnvelope<Connection2>]) {
            // Do something
        }
    }

    struct FooEventPublisher;

    // This event_publisher is interested in both server_config and connections.
    impl EventPublisher for FooEventPublisher {
        fn server_config(&mut self) -> Option<ServerConfigEventPublisher> {
            Some(Box::new(TestServerConfigEventPublisher))
        }

        fn connection(&mut self) -> Option<ConnectionEventPublisher2> {
            Some(Box::new(TestConnectionEventPublisher2))
        }
    }

    struct BarEventPublisher;

    // This event_publisher is only interested in connections.
    impl EventPublisher for BarEventPublisher {
        fn connection(&mut self) -> Option<ConnectionEventPublisher2> {
            Some(Box::new(TestConnectionEventPublisher2))
        }
    }

    #[test]
    fn test_partition_event_publishers() {
        let event_publishers: Vec<Box<dyn EventPublisher>> =
            vec![Box::new(FooEventPublisher), Box::new(BarEventPublisher)];

        let Partitions {
            connection_event_publishers,
            document_event_publishers,
            service_event_publishers,
            server_config_event_publishers,
            credential_event_publishers,
            offer_event_publishers,
            holder_credential_event_publishers,
            presentation_event_publishers,
            received_offer_event_publishers,
            authorization_request_event_publishers,
            connection2_event_publishers,
        } = partition_event_publishers(event_publishers);

        assert_eq!(connection_event_publishers.len(), 0);
        assert_eq!(document_event_publishers.len(), 0);
        assert_eq!(service_event_publishers.len(), 0);
        assert_eq!(server_config_event_publishers.len(), 1);
        assert_eq!(credential_event_publishers.len(), 0);
        assert_eq!(offer_event_publishers.len(), 0);
        assert_eq!(holder_credential_event_publishers.len(), 0);
        assert_eq!(presentation_event_publishers.len(), 0);
        assert_eq!(received_offer_event_publishers.len(), 0);
        assert_eq!(authorization_request_event_publishers.len(), 0);
        assert_eq!(connection2_event_publishers.len(), 2);
    }
}
