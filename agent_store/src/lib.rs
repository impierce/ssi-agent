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
use agent_authorization::services::{AuthorizationServices, OAuth2AuthorizationRequestDomainServices};
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
use agent_identity::connection::views::ConnectionView;
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
use agent_issuance::nonce::views::NonceView;
use agent_issuance::offer::views::all_offers::AllOffersView;
use agent_issuance::offer::views::OfferView;
use agent_issuance::public_offer::views::AllPublicOffersView;
use agent_issuance::public_offer::views::PublicOfferView;
use agent_issuance::server_config::views::ServerConfigView;
use agent_issuance::status_list::aggregate::StatusListAggregate;
use agent_issuance::status_list::views::all_status_lists::AllStatusListsView;
use agent_issuance::status_list::views::StatusListView;
use agent_issuance::SimpleLoggingQuery;
use agent_issuance::{
    credential::aggregate::Credential, nonce::aggregate::Nonce, offer::aggregate::Offer,
    public_offer::aggregate::PublicOffer, server_config::aggregate::ServerConfig,
};
use agent_library::catalog::aggregate::Catalog;
use agent_library::catalog::services::{CatalogServiceImpl, CatalogServices};
use agent_library::catalog::views::view_all_catalogs::AllCatalogsView;
use agent_library::catalog::views::CatalogView;
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
use cqrs_es::{Aggregate, CqrsFramework, DomainEvent, EventStore, Query, View};
use shared_kernel::authorization::AllowAllAuthorizationChecker;
use shared_kernel::view_repository::DynViewRepository;
use std::collections::HashMap;
use std::sync::Arc;

pub mod event_source;
pub mod event_verification;
pub mod in_memory;
pub mod mongodb;
pub mod postgres;

pub use event_source::MongoEventSource;
use shared_kernel::event_bus::EventBusHandle;

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
    Arc<dyn DynViewRepository<V, A>>,
    Arc<dyn DynViewRepository<AV, A>>,
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

fn bus_publisher<A: Aggregate + 'static>(bus: &EventBusHandle) -> Vec<Box<dyn Query<A>>>
where
    A::Event: serde::Serialize + DomainEvent,
{
    vec![Box::new(bus.clone()) as Box<dyn Query<A>>]
}

pub async fn identity_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    services: Arc<IdentityServices>,
    event_bus: EventBusHandle,
) -> IdentityState {
    let (connection_command_handler, connection, all_connections) = builder
        .commands_and_queries::<ConnectionView, Connection, AllConnectionsView>(
            services.clone(),
            bus_publisher(&event_bus),
        )
        .await;
    let (document_command_handler, document, all_documents) = builder
        .commands_and_queries::<Document, Document, AllDocumentsView>(services.clone(), bus_publisher(&event_bus))
        .await;
    let (profile_command_handler, profile, _all_profiles) = builder
        .commands_and_queries::<Profile, Profile, Profile>(services.clone(), bus_publisher(&event_bus))
        .await;
    let (service_command_handler, service, all_services) = builder
        .commands_and_queries::<Service, Service, AllServicesView>(services.clone(), bus_publisher(&event_bus))
        .await;

    IdentityState {
        authorization_checker: Arc::new(AllowAllAuthorizationChecker),
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
    event_bus: EventBusHandle,
    template_queries: Vec<Box<dyn Query<Template>>>,
) -> LibraryState {
    let mut queries: Vec<Box<dyn Query<Template>>> = bus_publisher(&event_bus);
    queries.extend(template_queries);

    let (template_command_handler, template, all_templates) = builder
        .commands_and_queries::<Template, Template, AllTemplatesView>((), queries)
        .await;

    let catalog_services: Arc<dyn CatalogServices> = Arc::new(CatalogServiceImpl {
        template_view_repo: template.clone(),
    });

    let (catalog_command_handler, catalog, all_catalogs) = builder
        .commands_and_queries::<CatalogView, Catalog, AllCatalogsView>(catalog_services, bus_publisher(&event_bus))
        .await;

    LibraryState {
        authorization_checker: Arc::new(AllowAllAuthorizationChecker),
        command: agent_library::state::CommandHandlers {
            template: template_command_handler,
            catalog: catalog_command_handler,
        },
        query: agent_library::state::ViewRepositories {
            template,
            all_templates,
            catalog,
            all_catalogs,
        },
    }
}

pub async fn authorization_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    services: Arc<AuthorizationServices>,
    event_bus: EventBusHandle,
    oauth2_authorization_request_domain_services: OAuth2AuthorizationRequestDomainServices,
) -> AuthorizationState {
    let (authorization_code_command_handler, authorization_code, _all_authorization_codes) = builder
        .commands_and_queries::<AuthorizationCodeView, AuthorizationCode, AllAuthorizationCodesView>(
            (),
            bus_publisher(&event_bus),
        )
        .await;
    let (client_command_handler, client, _all_clients) = builder
        .commands_and_queries::<ClientView, Client, AllClientsView>((), bus_publisher(&event_bus))
        .await;
    let (
        oauth2_authorization_request_command_handler,
        oauth2_authorization_request,
        _all_oauth2_authorization_requests,
    ) = builder
        .commands_and_queries::<
            OAuth2AuthorizationRequestView,
            OAuth2AuthorizationRequest,
            AllOAuth2AuthorizationRequestsView,
        >(
            oauth2_authorization_request_domain_services,
            bus_publisher(&event_bus),
        )
        .await;
    let (token_command_handler, access_token, _all_access_tokens) = builder
        .commands_and_queries::<AccessTokenView, AccessToken, AllAccessTokensView>((), bus_publisher(&event_bus))
        .await;

    AuthorizationState {
        authorization_checker: Arc::new(AllowAllAuthorizationChecker),
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
    event_bus: EventBusHandle,
) -> agent_issuance::state::IssuanceState {
    let (credential_command_handler, credential, all_credentials) = builder
        .commands_and_queries::<CredentialView, Credential, AllCredentialsView>(
            services.clone(),
            bus_publisher(&event_bus),
        )
        .await;
    let (offer_command_handler, offer, all_offers) = builder
        .commands_and_queries::<OfferView, Offer, AllOffersView>(services.clone(), bus_publisher(&event_bus))
        .await;
    let (public_offer_command_handler, public_offer, all_public_offers) = builder
        .commands_and_queries::<PublicOfferView, PublicOffer, AllPublicOffersView>(
            services.clone(),
            bus_publisher(&event_bus),
        )
        .await;
    let (server_config_command_handler, server_config, _all_server_configs) = builder
        .commands_and_queries::<ServerConfigView, ServerConfig, ServerConfig>(
            services.clone(),
            bus_publisher(&event_bus),
        )
        .await;
    let (nonce_command_handler, nonce, _) = builder
        .commands_and_queries::<NonceView, Nonce, NonceView>(services.clone(), bus_publisher(&event_bus))
        .await;
    let (status_list_command_handler, status_list, all_status_lists) = builder
        .commands_and_queries::<StatusListView, StatusListAggregate, AllStatusListsView>(
            services.clone(),
            bus_publisher(&event_bus),
        )
        .await;

    agent_issuance::state::IssuanceState {
        authorization_checker: Arc::new(AllowAllAuthorizationChecker),
        command: agent_issuance::state::CommandHandlers {
            credential: credential_command_handler,
            offer: offer_command_handler,
            public_offer: public_offer_command_handler,
            server_config: server_config_command_handler,
            nonce: nonce_command_handler,
            status_list: status_list_command_handler,
        },
        query: agent_issuance::state::ViewRepositories {
            server_config,
            credential,
            all_credentials,
            offer,
            all_offers,
            public_offer,
            all_public_offers,
            nonce,
            status_list,
            all_status_lists,
        },
        subject: services.issuer.clone(),
    }
}

pub async fn verification_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    services: Arc<VerificationServices>,
    event_bus: EventBusHandle,
) -> VerificationState {
    let (authorization_request_command_handler, authorization_request, all_authorization_requests) = builder
        .commands_and_queries::<AuthorizationRequest, AuthorizationRequest, AllAuthorizationRequestsView>(
            services.clone(),
            bus_publisher(&event_bus),
        )
        .await;

    VerificationState {
        authorization_checker: Arc::new(AllowAllAuthorizationChecker),
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
    event_bus: EventBusHandle,
) -> HolderState {
    let (holder_credential_command_handler, holder_credential, all_holder_credential) = builder
        .commands_and_queries::<HolderCredential, HolderCredential, AllHolderCredentialsView>(
            services.clone(),
            bus_publisher(&event_bus),
        )
        .await;

    let (presentation_command_handler, presentation, all_presentations) = builder
        .commands_and_queries::<Presentation, Presentation, AllPresentationsView>(
            services.clone(),
            bus_publisher(&event_bus),
        )
        .await;

    let (received_offer_command_handler, received_offer, all_received_offers) = builder
        .commands_and_queries::<ReceivedOffer, ReceivedOffer, AllReceivedOffersView>(
            services.clone(),
            bus_publisher(&event_bus),
        )
        .await;

    HolderState {
        authorization_checker: Arc::new(AllowAllAuthorizationChecker),
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
