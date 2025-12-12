use agent_authorization::domain::access_token::aggregate::AccessToken;
use agent_authorization::domain::access_token::views::all_tokens::AllAccessTokensView;
use agent_authorization::domain::access_token::views::AccessTokenView;
use agent_authorization::domain::authorization_code::aggregate::AuthorizationCode;
use agent_authorization::domain::authorization_code::views::all_authorization_codes::AllAuthorizationCodesView;
use agent_authorization::domain::authorization_code::views::AuthorizationCodeView;
use agent_authorization::domain::client::aggregate::Client;
use agent_authorization::domain::client::views::all_clients::AllClientsView;
use agent_authorization::domain::client::views::ClientView;
use agent_authorization::domain::oauth2_authorization_request::aggregate::OAuth2AuthorizationRequest;
use agent_authorization::domain::oauth2_authorization_request::views::all_oauth2_authorization_requests::AllOAuth2AuthorizationRequestsView;
use agent_authorization::domain::oauth2_authorization_request::views::OAuth2AuthorizationRequestView;
use agent_authorization::services::AuthorizationServices;
use agent_authorization::state::AuthorizationState;
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
use agent_issuance::credential::views::all_credentials::AllCredentialsView;
use agent_issuance::credential::views::CredentialView;
use agent_issuance::offer::views::all_offers::AllOffersView;
use agent_issuance::offer::views::OfferView;
use agent_issuance::server_config::views::ServerConfigView;
use agent_issuance::SimpleLoggingQuery;
use agent_issuance::{
    credential::aggregate::Credential, offer::aggregate::Offer, server_config::aggregate::ServerConfig,
};
use agent_library::state::LibraryState;
use agent_library::template::aggregate::Template;
use agent_library::template::views::all_templates::AllTemplatesView;
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
pub mod mongodb;
pub mod postgres;

/// A generic command handler for a specific aggregate.
///
/// This struct wraps the `CqrsFramework` to provide a unified entry point
/// for executing commands and configuring query-side processors (views).
pub struct AggregateHandler<A, CCB>
where
    A: Aggregate,
    CCB: EventStore<A> + Send + Sync + 'static,
{
    pub cqrs: CqrsFramework<A, CCB>,
}

/// Implements the `Command` trait to allow the handler to execute commands.
///
/// This implementation simply delegates the call to the underlying `CqrsFramework`.
#[async_trait]
impl<A, CCB> Command<A> for AggregateHandler<A, CCB>
where
    A: Aggregate,
    CCB: EventStore<A>,
    <CCB as EventStore<A>>::AC: Send,
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

impl<A, CCB> AggregateHandler<A, CCB>
where
    A: Aggregate + 'static,
    CCB: EventStore<A>,
    <A as Aggregate>::Command: Send,
{
    /// Appends a query processor (e.g., a view generator) to the CQRS framework.
    ///
    /// This is used to register components that listen to events and update read models.
    fn append_query<Q>(self, query: Q) -> Self
    where
        Q: Query<A> + 'static,
    {
        Self {
            cqrs: self.cqrs.append_query(Box::new(query)),
        }
    }

    /// Appends a dynamically dispatched event publisher.
    fn append_event_publisher(self, query: Box<dyn Query<A>>) -> Self {
        Self {
            cqrs: self.cqrs.append_query(query),
        }
    }

    /// A convenience method to configure the handler with standard queries and custom event publishers.
    ///
    /// This wires up the default queries for logging, single-aggregate views, and all-aggregate views,
    /// and then folds in any additional event publishers provided.
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
                .append_query(ListAllQuery::new(all_aggregates.clone(), all_aggregates_name)),
            |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
        )
    }
}

/// A type alias for the tuple of CQRS components for a given aggregate.
///
/// This includes the command handler, the single-instance view repository,
/// and the all-instances view repository.
pub type CqrsComponents<A, V, AV> = (
    Arc<dyn Command<A> + Send + Sync>,
    Arc<dyn ViewRepository<V, A>>,
    Arc<dyn ViewRepository<AV, A>>,
);

/// A trait for building the command and query infrastructure for a given aggregate.
///
/// Implementors of this trait (e.g., `InMemory`, `Postgres`) are responsible
/// for creating the full set of components needed to interact with an aggregate,
/// including the command handler and view repositories.
pub trait CqrsComponentBuilder {
    fn commands_and_queries<V: View<A> + 'static, A: Aggregate + 'static, AV: View<A> + 'static>(
        &self,
        identity_services: A::Services,
        event_publishers: Vec<Box<dyn Query<A>>>,
    ) -> impl std::future::Future<Output = CqrsComponents<A, V, AV>> + Send
    where
        <A as Aggregate>::Command: Send + Sync;
}

pub async fn identity_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
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

    let (connection_command_handler, connection, all_connections) = builder
        .commands_and_queries::<Connection, Connection, AllConnectionsView>(
            services.clone(),
            connection_event_publishers,
        )
        .await;
    let (document_command_handler, document, all_documents) = builder
        .commands_and_queries::<Document, Document, AllDocumentsView>(services.clone(), document_event_publishers)
        .await;
    let (profile_command_handler, profile, _all_profiles) = builder
        .commands_and_queries::<Profile, Profile, Profile>(services.clone(), vec![])
        .await;
    let (service_command_handler, service, all_services) = builder
        .commands_and_queries::<Service, Service, AllServicesView>(services.clone(), service_event_publishers)
        .await;

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

pub async fn library_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    event_publishers: Vec<Box<dyn EventPublisher>>,
    template_policies: Vec<Box<dyn Query<Template>>>,
) -> LibraryState {
    // Partition the event_publishers into the different aggregates.
    let Partitions {
        template_event_publishers: mut queries,
        ..
    } = partition_event_publishers(event_publishers);

    for policy in template_policies {
        queries.push(policy);
    }

    let (template_command_handler, template, all_templates) = builder
        .commands_and_queries::<Template, Template, AllTemplatesView>((), queries)
        .await;

    LibraryState {
        command: agent_library::state::CommandHandlers {
            template: template_command_handler,
        },
        query: agent_library::state::ViewRepositories {
            template,
            all_templates,
        },
    }
}

pub async fn authorization_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    services: Arc<AuthorizationServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> AuthorizationState {
    // Partition the event_publishers into the different aggregates.
    let Partitions {
        authorization_code_event_publishers,
        client_event_publishers,
        oauth2_authorization_request_event_publishers,
        access_token_event_publishers: token_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    let (authorization_code_command_handler, authorization_code, _all_authorization_codes) = builder
        .commands_and_queries::<AuthorizationCodeView, AuthorizationCode, AllAuthorizationCodesView>(
            (),
            authorization_code_event_publishers,
        )
        .await;
    let (client_command_handler, client, _all_clients) = builder
        .commands_and_queries::<ClientView, Client, AllClientsView>((), client_event_publishers)
        .await;
    let (
        oauth2_authorization_request_command_handler,
        oauth2_authorization_request,
        _all_oauth2_authorization_requests,
    ) = builder.commands_and_queries::<
        OAuth2AuthorizationRequestView,
        OAuth2AuthorizationRequest,
        AllOAuth2AuthorizationRequestsView,
    >((), oauth2_authorization_request_event_publishers)
    .await;
    let (token_command_handler, access_token, _all_access_tokens) = builder
        .commands_and_queries::<AccessTokenView, AccessToken, AllAccessTokensView>((), token_event_publishers)
        .await;

    AuthorizationState {
        command: agent_authorization::state::CommandHandlers {
            authorization_code: authorization_code_command_handler,
            client: client_command_handler,
            oauth2_authorization_request: oauth2_authorization_request_command_handler,
            access_token: token_command_handler,
        },
        query: agent_authorization::state::ViewRepositories {
            client,
            oauth2_authorization_request,
            authorization_code,
            access_token,
        },
        signer: services.signer.clone(),
    }
}

pub async fn issuance_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    services: Arc<agent_issuance::services::IssuanceServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> agent_issuance::state::IssuanceState {
    // Partition the event_publishers into the different aggregates.
    let Partitions {
        credential_event_publishers,
        offer_event_publishers,
        server_config_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    let (credential_command_handler, credential, all_credentials) = builder
        .commands_and_queries::<CredentialView, Credential, AllCredentialsView>(
            services.clone(),
            credential_event_publishers,
        )
        .await;
    let (offer_command_handler, offer, all_offers) = builder
        .commands_and_queries::<OfferView, Offer, AllOffersView>(services.clone(), offer_event_publishers)
        .await;
    let (server_config_command_handler, server_config, _all_server_configs) = builder
        .commands_and_queries::<ServerConfigView, ServerConfig, ServerConfig>(
            services.clone(),
            server_config_event_publishers,
        )
        .await;

    agent_issuance::state::IssuanceState {
        command: agent_issuance::state::CommandHandlers {
            credential: credential_command_handler,
            offer: offer_command_handler,
            server_config: server_config_command_handler,
        },
        query: agent_issuance::state::ViewRepositories {
            server_config,
            credential,
            all_credentials,
            offer,
            all_offers,
        },
        subject: services.issuer.clone(),
    }
}

pub async fn verification_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    services: Arc<VerificationServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> VerificationState {
    // Partition the event_publishers into the different aggregates.
    let Partitions {
        authorization_request_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    let (authorization_request_command_handler, authorization_request, all_authorization_requests) = builder
        .commands_and_queries::<AuthorizationRequest, AuthorizationRequest, AllAuthorizationRequestsView>(
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

pub async fn holder_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
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

    let (holder_credential_command_handler, holder_credential, all_holder_credential) = builder
        .commands_and_queries::<HolderCredential, HolderCredential, AllHolderCredentialsView>(
            services.clone(),
            holder_credential_publisher,
        )
        .await;

    let (presentation_command_handler, presentation, all_presentations) = builder
        .commands_and_queries::<Presentation, Presentation, AllPresentationsView>(
            services.clone(),
            presentation_event_publishers,
        )
        .await;

    let (received_offer_command_handler, received_offer, all_received_offers) = builder
        .commands_and_queries::<ReceivedOffer, ReceivedOffer, AllReceivedOffersView>(
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
pub type TemplateEventPublisher = Box<dyn Query<Template>>;
pub type AuthorizationCodeEventPublisher = Box<dyn Query<AuthorizationCode>>;
pub type ClientEventPublisher = Box<dyn Query<Client>>;
pub type OAuth2AuthorizationRequestEventPublisher = Box<dyn Query<OAuth2AuthorizationRequest>>;
pub type AccessTokenEventPublisher = Box<dyn Query<AccessToken>>;
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
    pub template_event_publishers: Vec<TemplateEventPublisher>,
    pub authorization_code_event_publishers: Vec<AuthorizationCodeEventPublisher>,
    pub client_event_publishers: Vec<ClientEventPublisher>,
    pub oauth2_authorization_request_event_publishers: Vec<OAuth2AuthorizationRequestEventPublisher>,
    pub access_token_event_publishers: Vec<AccessTokenEventPublisher>,
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
    fn connection(&mut self) -> Option<ConnectionEventPublisher>;
    fn document(&mut self) -> Option<DocumentEventPublisher>;
    fn profile(&mut self) -> Option<ProfileEventPublisher>;
    fn service(&mut self) -> Option<ServiceEventPublisher>;

    fn template(&mut self) -> Option<TemplateEventPublisher>;

    fn authorization_code(&mut self) -> Option<AuthorizationCodeEventPublisher>;
    fn client(&mut self) -> Option<ClientEventPublisher>;
    fn oauth2_authorization_request(&mut self) -> Option<OAuth2AuthorizationRequestEventPublisher>;
    fn access_token(&mut self) -> Option<AccessTokenEventPublisher>;

    fn server_config(&mut self) -> Option<ServerConfigEventPublisher>;
    fn credential(&mut self) -> Option<CredentialEventPublisher>;
    fn offer(&mut self) -> Option<OfferEventPublisher>;

    fn holder_credential(&mut self) -> Option<HolderCredentialEventPublisher>;
    fn presentation(&mut self) -> Option<PresentationEventPublisher>;
    fn received_offer(&mut self) -> Option<ReceivedOfferEventPublisher>;

    fn authorization_request(&mut self) -> Option<AuthorizationRequestEventPublisher>;
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

            if let Some(template) = event_publisher.template() {
                partitions.template_event_publishers.push(template);
            }

            if let Some(authorization_code) = event_publisher.authorization_code() {
                partitions.authorization_code_event_publishers.push(authorization_code);
            }
            if let Some(client) = event_publisher.client() {
                partitions.client_event_publishers.push(client);
            }
            if let Some(oauth2_authorization_request) = event_publisher.oauth2_authorization_request() {
                partitions
                    .oauth2_authorization_request_event_publishers
                    .push(oauth2_authorization_request);
            }
            if let Some(access_token) = event_publisher.access_token() {
                partitions.access_token_event_publishers.push(access_token);
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
        fn connection(&mut self) -> Option<ConnectionEventPublisher> {
            Some(Box::new(TestConnectionEventPublisher))
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

        fn template(&mut self) -> Option<TemplateEventPublisher> {
            None
        }

        fn authorization_code(&mut self) -> Option<AuthorizationCodeEventPublisher> {
            None
        }
        fn client(&mut self) -> Option<ClientEventPublisher> {
            None
        }
        fn oauth2_authorization_request(&mut self) -> Option<OAuth2AuthorizationRequestEventPublisher> {
            None
        }
        fn access_token(&mut self) -> Option<AccessTokenEventPublisher> {
            None
        }

        fn server_config(&mut self) -> Option<ServerConfigEventPublisher> {
            Some(Box::new(TestServerConfigEventPublisher))
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

    struct BarEventPublisher;

    // This event_publisher is only interested in connections.
    impl EventPublisher for BarEventPublisher {
        fn connection(&mut self) -> Option<ConnectionEventPublisher> {
            Some(Box::new(TestConnectionEventPublisher))
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

        fn template(&mut self) -> Option<TemplateEventPublisher> {
            None
        }

        fn authorization_code(&mut self) -> Option<AuthorizationCodeEventPublisher> {
            None
        }
        fn client(&mut self) -> Option<ClientEventPublisher> {
            None
        }
        fn oauth2_authorization_request(&mut self) -> Option<OAuth2AuthorizationRequestEventPublisher> {
            None
        }
        fn access_token(&mut self) -> Option<AccessTokenEventPublisher> {
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

    #[test]
    fn test_partition_event_publishers() {
        let event_publishers: Vec<Box<dyn EventPublisher>> =
            vec![Box::new(FooEventPublisher), Box::new(BarEventPublisher)];

        let Partitions {
            connection_event_publishers,
            document_event_publishers,
            profile_event_publishers,
            service_event_publishers,
            template_event_publishers,
            authorization_code_event_publishers,
            client_event_publishers,
            oauth2_authorization_request_event_publishers,
            access_token_event_publishers: token_event_publishers,
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
        assert_eq!(template_event_publishers.len(), 0);
        assert_eq!(authorization_code_event_publishers.len(), 0);
        assert_eq!(client_event_publishers.len(), 0);
        assert_eq!(oauth2_authorization_request_event_publishers.len(), 0);
        assert_eq!(token_event_publishers.len(), 0);
        assert_eq!(server_config_event_publishers.len(), 1);
        assert_eq!(credential_event_publishers.len(), 0);
        assert_eq!(offer_event_publishers.len(), 0);
        assert_eq!(holder_credential_event_publishers.len(), 0);
        assert_eq!(presentation_event_publishers.len(), 0);
        assert_eq!(received_offer_event_publishers.len(), 0);
        assert_eq!(authorization_request_event_publishers.len(), 0);
    }
}
