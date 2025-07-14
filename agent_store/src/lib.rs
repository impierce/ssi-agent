use agent_holder::credential::aggregate::Credential as HolderCredential;
use agent_holder::credential::queries::all_credentials::AllHolderCredentialsView;
use agent_holder::offer::aggregate::Offer as ReceivedOffer;
use agent_holder::offer::queries::all_offers::AllReceivedOffersView;
use agent_holder::presentation::aggregate::Presentation;
use agent_holder::presentation::views::all_presentations::AllPresentationsView;
use agent_holder::services::HolderServices;
use agent_holder::state::HolderState;
use agent_identity::connection::views::all_connections::AllConnectionsView;
use agent_identity::document::views::all_documents::AllDocumentsView;
use agent_identity::service::views::all_services::AllServicesView;
use agent_identity::services::IdentityServices;
use agent_identity::state::IdentityState;
use agent_identity::{
    connection::aggregate::Connection, document::aggregate::Document, profile::aggregate::Profile,
    service::aggregate::Service,
};
use agent_issuance::SimpleLoggingQuery;
use agent_issuance::{
    credential::aggregate::Credential, offer::aggregate::Offer, server_config::aggregate::ServerConfig,
};
use agent_shared::application_state::Command;
use agent_shared::custom_queries::ListAllQuery;
use agent_shared::generic_query::generic_query;
use agent_verification::authorization_request::aggregate::AuthorizationRequest;
use agent_verification::authorization_request::views::all_authorization_requests::AllAuthorizationRequestsView;
use agent_verification::services::VerificationServices;
use agent_verification::state::VerificationState;
use async_trait::async_trait;
use cqrs_es::persist::ViewRepository;
use cqrs_es::{Aggregate, CqrsFramework, EventStore, Query, View};
use std::collections::HashMap;
use std::sync::Arc;

pub mod in_memory;
pub mod postgres;

pub struct AggregateHandler<A, ES>
where
    A: Aggregate,
    ES: EventStore<A> + Send + Sync + 'static,
{
    pub cqrs: CqrsFramework<A, ES>,
}

#[async_trait]
impl<A, ES> Command<A> for AggregateHandler<A, ES>
where
    A: Aggregate,
    ES: EventStore<A>,
    <ES as EventStore<A>>::AC: Send,
    <A as Aggregate>::Command: Send,
{
    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: A::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<A::Error>> {
        self.cqrs.execute_with_metadata(aggregate_id, command, metadata).await
    }
}

impl<A, ES> AggregateHandler<A, ES>
where
    A: Aggregate + 'static,
    ES: EventStore<A>,
    <A as Aggregate>::Command: Send,
{
    fn append_query<Q>(self, query: Q) -> Self
    where
        Q: Query<A> + 'static,
    {
        Self {
            cqrs: self.cqrs.append_query(Box::new(query)),
        }
    }

    fn append_event_publisher(self, query: Box<dyn Query<A>>) -> Self {
        Self {
            cqrs: self.cqrs.append_query(query),
        }
    }

    fn with_parameters<V, AV, VR1, VR2>(
        self,
        aggregate: Arc<VR1>,
        all_aggregates: Arc<VR2>,
        event_publishers: Vec<Box<dyn Query<A>>>,
        all_aggregates_name: &str,
    ) -> Self
    where
        V: View<A> + 'static,
        AV: View<A> + 'static,
        VR1: ViewRepository<V, A> + 'static,
        VR2: ViewRepository<AV, A> + 'static,
    {
        event_publishers.into_iter().fold(
            self.append_query(SimpleLoggingQuery {})
                .append_query(generic_query(aggregate.clone()))
                .append_query(ListAllQuery::new(all_aggregates.clone(), &all_aggregates_name)),
            |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
        )
    }
}

pub trait EventStoreTemp {
    fn commands_and_queries<V: View<A> + 'static, A: Aggregate + 'static, AV: View<A> + 'static>(
        identity_services: A::Services,
        event_publishers: Vec<Box<dyn Query<A>>>,
    ) -> impl std::future::Future<
        Output = (
            Arc<dyn Command<A> + Send + Sync>,
            Arc<dyn ViewRepository<V, A>>,
            Arc<dyn ViewRepository<AV, A>>,
        ),
    > + Send
    where
        <A as Aggregate>::Command: Send + Sync;
}

pub async fn identity_state<ES: EventStoreTemp>(
    services: Arc<IdentityServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> IdentityState {
    // Partition the event_publishers into the different aggregates.
    let Partitions {
        connection_event_publishers,
        document_event_publishers,
        service_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    let (connection_command_handler, connection, all_connections) = ES::commands_and_queries::<
        Connection,
        Connection,
        AllConnectionsView,
    >(services.clone(), connection_event_publishers)
    .await;
    let (document_command_handler, document, all_documents) =
        ES::commands_and_queries::<Document, Document, AllDocumentsView>(services.clone(), document_event_publishers)
            .await;
    let (profile_command_handler, profile, all_profiles) =
        ES::commands_and_queries::<Profile, Profile, Profile>(services.clone(), vec![]).await;
    let (service_command_handler, service, all_services) =
        ES::commands_and_queries::<Service, Service, AllServicesView>(services.clone(), service_event_publishers).await;

    IdentityState {
        command: agent_identity::state::CommandHandlers {
            connection: connection_command_handler,
            document: document_command_handler,
            profile: profile_command_handler,
            service: service_command_handler,
        },
        query: agent_identity::state::ViewRepositories {
            connection,
            all_connections,
            document,
            all_documents,
            service,
            all_services,
            profile,
        },
    }
}

pub async fn verification_state<ES: EventStoreTemp>(
    services: Arc<VerificationServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> VerificationState {
    // Partition the event_publishers into the different aggregates.
    let Partitions {
        authorization_request_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    let (authorization_request_command_handler, authorization_request, all_authorization_requests) =
        ES::commands_and_queries::<AuthorizationRequest, AuthorizationRequest, AllAuthorizationRequestsView>(
            services.clone(),
            authorization_request_event_publishers,
        )
        .await;

    VerificationState {
        command: agent_verification::state::CommandHandlers {
            authorization_request: authorization_request_command_handler,
        },
        query: agent_verification::state::ViewRepositories {
            authorization_request,
            all_authorization_requests,
        },
    }
}

pub async fn holder_state<ES: EventStoreTemp>(
    services: Arc<HolderServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> HolderState {
    // Partition the event_publishers into the different aggregates.
    let Partitions {
        holder_credential_event_publishers: holder_credential_publisher,
        presentation_event_publishers,
        received_offer_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    let (holder_credential_command_handler, holder_credential, all_holder_credential) =
        ES::commands_and_queries::<HolderCredential, HolderCredential, AllHolderCredentialsView>(
            services.clone(),
            holder_credential_publisher,
        )
        .await;

    let (presentation_command_handler, presentation, all_presentations) =
        ES::commands_and_queries::<Presentation, Presentation, AllPresentationsView>(
            services.clone(),
            presentation_event_publishers,
        )
        .await;

    let (received_offer_command_handler, received_offer, all_received_offers) =
        ES::commands_and_queries::<ReceivedOffer, ReceivedOffer, AllReceivedOffersView>(
            services.clone(),
            received_offer_event_publishers,
        )
        .await;

    HolderState {
        command: agent_holder::state::CommandHandlers {
            credential: holder_credential_command_handler,
            presentation: presentation_command_handler,
            offer: received_offer_command_handler,
        },
        query: agent_holder::state::ViewRepositories {
            holder_credential,
            all_holder_credentials: all_holder_credential,
            presentation,
            all_presentations,
            received_offer,
            all_received_offers,
        },
    }
}

pub type ConnectionEventPublisher = Box<dyn Query<Connection>>;
pub type DocumentEventPublisher = Box<dyn Query<Document>>;
pub type ProfileEventPublisher = Box<dyn Query<Profile>>;
pub type ServiceEventPublisher = Box<dyn Query<Service>>;
pub type ServerConfigEventPublisher = Box<dyn Query<ServerConfig>>;
pub type CredentialEventPublisher = Box<dyn Query<Credential>>;
pub type OfferEventPublisher = Box<dyn Query<Offer>>;
pub type HolderCredentialEventPublisher = Box<dyn Query<agent_holder::credential::aggregate::Credential>>;
pub type PresentationEventPublisher = Box<dyn Query<agent_holder::presentation::aggregate::Presentation>>;
pub type ReceivedOfferEventPublisher = Box<dyn Query<agent_holder::offer::aggregate::Offer>>;
pub type AuthorizationRequestEventPublisher = Box<dyn Query<AuthorizationRequest>>;

/// Contains all the event_publishers for each aggregate.
#[derive(Default)]
pub struct Partitions {
    pub connection_event_publishers: Vec<ConnectionEventPublisher>,
    pub document_event_publishers: Vec<DocumentEventPublisher>,
    pub profile_event_publishers: Vec<ProfileEventPublisher>,
    pub service_event_publishers: Vec<ServiceEventPublisher>,
    pub server_config_event_publishers: Vec<ServerConfigEventPublisher>,
    pub credential_event_publishers: Vec<CredentialEventPublisher>,
    pub offer_event_publishers: Vec<OfferEventPublisher>,
    pub holder_credential_event_publishers: Vec<HolderCredentialEventPublisher>,
    pub presentation_event_publishers: Vec<PresentationEventPublisher>,
    pub received_offer_event_publishers: Vec<ReceivedOfferEventPublisher>,
    pub authorization_request_event_publishers: Vec<AuthorizationRequestEventPublisher>,
}

/// An outbound event_publisher is a component that listens to events and dispatches them to the appropriate service. For each
/// aggregate, by default, `None` is returned. If an event_publisher is interested in a specific aggregate, it should return a
/// `Some` with the appropriate query.
// TODO: move this to a separate crate that will include all the logic for event_publishers, i.e. `agent_event_publisher`.
pub trait EventPublisher {
    fn connection(&mut self) -> Option<ConnectionEventPublisher> {
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

    fn authorization_request(&mut self) -> Option<AuthorizationRequestEventPublisher> {
        None
    }
}

pub(crate) fn partition_event_publishers(event_publishers: Vec<Box<dyn EventPublisher>>) -> Partitions {
    event_publishers
        .into_iter()
        .fold(Partitions::default(), |mut partitions, mut event_publisher| {
            if let Some(connection) = event_publisher.connection() {
                partitions.connection_event_publishers.push(connection);
            }
            if let Some(document) = event_publisher.document() {
                partitions.document_event_publishers.push(document);
            }
            if let Some(profile) = event_publisher.profile() {
                partitions.profile_event_publishers.push(profile);
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

        let Partitions {
            connection_event_publishers,
            document_event_publishers,
            profile_event_publishers,
            service_event_publishers,
            server_config_event_publishers,
            credential_event_publishers,
            offer_event_publishers,
            holder_credential_event_publishers,
            presentation_event_publishers,
            received_offer_event_publishers,
            authorization_request_event_publishers,
        } = partition_event_publishers(event_publishers);

        assert_eq!(connection_event_publishers.len(), 2);
        assert_eq!(document_event_publishers.len(), 0);
        assert_eq!(profile_event_publishers.len(), 0);
        assert_eq!(service_event_publishers.len(), 0);
        assert_eq!(server_config_event_publishers.len(), 1);
        assert_eq!(credential_event_publishers.len(), 0);
        assert_eq!(offer_event_publishers.len(), 0);
        assert_eq!(holder_credential_event_publishers.len(), 0);
        assert_eq!(presentation_event_publishers.len(), 0);
        assert_eq!(received_offer_event_publishers.len(), 0);
        assert_eq!(authorization_request_event_publishers.len(), 0);
    }
}
