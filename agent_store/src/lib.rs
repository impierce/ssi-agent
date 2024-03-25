use agent_issuance::{
    credential::aggregate::Credential, offer::aggregate::Offer, server_config::aggregate::ServerConfig,
};
use agent_verification::{authorization_request::aggregate::AuthorizationRequest, connection::aggregate::Connection};
use cqrs_es::Query;

pub mod in_memory;
pub mod postgres;

pub type ServerConfigAdapter = Box<dyn Query<ServerConfig>>;
pub type CredentialAdapter = Box<dyn Query<Credential>>;
pub type OfferAdapter = Box<dyn Query<Offer>>;
pub type AuthorizationRequestAdapter = Box<dyn Query<AuthorizationRequest>>;
pub type ConnectionAdapter = Box<dyn Query<Connection>>;

/// Contains all the adapters for each aggregate.
pub type Partitions = (
    Vec<ServerConfigAdapter>,
    Vec<CredentialAdapter>,
    Vec<OfferAdapter>,
    Vec<AuthorizationRequestAdapter>,
    Vec<ConnectionAdapter>,
);

/// An outbound adapter is a component that listens to events and dispatches them to the appropriate service. For each
/// aggregate, by default, `None` is returned. If an adapter is interested in a specific aggregate, it should return a
/// `Some` with the appropriate query.
// TODO: move this to a separate crate that will include all the logic for adapters, i.e. `agent_adapter`.
pub trait OutboundAdapter {
    fn server_config(&mut self) -> Option<ServerConfigAdapter> {
        None
    }
    fn credential(&mut self) -> Option<Box<dyn Query<Credential>>> {
        None
    }
    fn offer(&mut self) -> Option<OfferAdapter> {
        None
    }

    fn connection(&mut self) -> Option<ConnectionAdapter> {
        None
    }
    fn authorization_request(&mut self) -> Option<AuthorizationRequestAdapter> {
        None
    }
}

pub(crate) fn partition_adapters(outbound_adapters: Vec<Box<dyn OutboundAdapter>>) -> Partitions {
    outbound_adapters.into_iter().fold(
        (vec![], vec![], vec![], vec![], vec![]),
        |mut partitions, mut adapter| {
            if let Some(server_config) = adapter.server_config() {
                partitions.0.push(server_config);
            }
            if let Some(credential) = adapter.credential() {
                partitions.1.push(credential);
            }
            if let Some(offer) = adapter.offer() {
                partitions.2.push(offer);
            }

            if let Some(authorization_request) = adapter.authorization_request() {
                partitions.3.push(authorization_request);
            }
            if let Some(connection) = adapter.connection() {
                partitions.4.push(connection);
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

    struct TestServerConfigAdapter;

    #[async_trait]
    impl Query<ServerConfig> for TestServerConfigAdapter {
        async fn dispatch(&self, _aggregate_id: &str, _events: &[EventEnvelope<ServerConfig>]) {
            // Do something
        }
    }

    struct TestConnectionAdapter;

    #[async_trait]
    impl Query<Connection> for TestConnectionAdapter {
        async fn dispatch(&self, _aggregate_id: &str, _events: &[EventEnvelope<Connection>]) {
            // Do something
        }
    }

    struct FooAdapter;

    // This adapter is interested in both server_config and connections.
    impl OutboundAdapter for FooAdapter {
        fn server_config(&mut self) -> Option<ServerConfigAdapter> {
            Some(Box::new(TestServerConfigAdapter))
        }

        fn connection(&mut self) -> Option<ConnectionAdapter> {
            Some(Box::new(TestConnectionAdapter))
        }
    }

    struct BarAdapter;

    // This adapter is only interested in connections.
    impl OutboundAdapter for BarAdapter {
        fn connection(&mut self) -> Option<ConnectionAdapter> {
            Some(Box::new(TestConnectionAdapter))
        }
    }

    #[test]
    fn test_partition_adapters() {
        let adapters: Vec<Box<dyn OutboundAdapter>> = vec![Box::new(FooAdapter), Box::new(BarAdapter)];

        let (
            server_config_adapters,
            credential_adapters,
            offer_adapters,
            authorization_request_adapters,
            connection_adapters,
        ) = partition_adapters(adapters);

        assert_eq!(server_config_adapters.len(), 1);
        assert_eq!(credential_adapters.len(), 0);
        assert_eq!(offer_adapters.len(), 0);
        assert_eq!(authorization_request_adapters.len(), 0);
        assert_eq!(connection_adapters.len(), 2);
    }
}
