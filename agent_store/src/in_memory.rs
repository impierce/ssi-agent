use crate::{partition_event_publishers, EventPublisher, Partitions};
use agent_holder::{services::HolderServices, state::HolderState};
use agent_identity::{services::IdentityServices, state::IdentityState};
use agent_issuance::{
    offer::{
        aggregate::Offer,
        queries::{
            access_token::{AccessTokenQuery, AccessTokenView},
            pre_authorized_code::{PreAuthorizedCodeQuery, PreAuthorizedCodeView},
        },
    },
    services::IssuanceServices,
    state::IssuanceState,
    SimpleLoggingQuery,
};
use agent_shared::{application_state::Command, custom_queries::ListAllQuery, generic_query::generic_query};
use agent_verification::{services::VerificationServices, state::VerificationState};
use async_trait::async_trait;
use cqrs_es::{
    mem_store::MemStore,
    persist::{PersistenceError, ViewContext, ViewRepository},
    Aggregate, CqrsFramework, Query, View,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

#[derive(Default)]
struct MemRepository<V: View<A>, A: Aggregate> {
    pub map: Mutex<HashMap<String, serde_json::Value>>,
    _phantom: std::marker::PhantomData<(V, A)>,
}

impl<V: View<A>, A: Aggregate> MemRepository<V, A> {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl<V, A> ViewRepository<V, A> for MemRepository<V, A>
where
    V: View<A>,
    A: Aggregate,
{
    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
        Ok(self
            .map
            .lock()
            .await
            .get(view_id)
            .map(|view| serde_json::from_value(view.clone()).unwrap()))
    }

    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError> {
        Ok(self.map.lock().await.get(view_id).map(|view| {
            let view = serde_json::from_value(view.clone()).unwrap();
            let view_context = ViewContext::new(view_id.to_string(), 0);
            (view, view_context)
        }))
    }

    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError> {
        let payload = serde_json::to_value(&view).unwrap();
        self.map.lock().await.insert(context.view_instance_id, payload);
        Ok(())
    }
}

struct AggregateHandler<A>
where
    A: Aggregate,
{
    pub cqrs: CqrsFramework<A, MemStore<A>>,
}

#[async_trait]
impl<A> Command<A> for AggregateHandler<A>
where
    A: Aggregate,
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

impl<A> AggregateHandler<A>
where
    A: Aggregate,
    <A as Aggregate>::Command: Send,
{
    fn new(services: A::Services) -> Self {
        Self {
            cqrs: CqrsFramework::new(MemStore::default(), vec![], services),
        }
    }

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
}

pub async fn identity_state(
    identity_services: Arc<IdentityServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> IdentityState {
    // Initialize the in-memory repositories.
    let document = Arc::new(MemRepository::default());
    let service = Arc::new(MemRepository::default());
    let all_services = Arc::new(MemRepository::default());

    // Create custom-queries for the offer aggregate.
    let all_services_query = ListAllQuery::new(all_services.clone(), "all_services");

    // Partition the event_publishers into the different aggregates.
    let Partitions {
        document_event_publishers,
        service_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    IdentityState {
        command: agent_identity::state::CommandHandlers {
            document: Arc::new(
                document_event_publishers.into_iter().fold(
                    AggregateHandler::new(identity_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(document.clone())),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            service: Arc::new(
                service_event_publishers.into_iter().fold(
                    AggregateHandler::new(identity_services)
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(service.clone()))
                        .append_query(all_services_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
        },
        query: agent_identity::state::ViewRepositories {
            document,
            service,
            all_services,
        },
    }
}

pub async fn issuance_state(
    issuance_services: Arc<IssuanceServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> IssuanceState {
    // Initialize the in-memory repositories.
    let server_config = Arc::new(MemRepository::default());
    let pre_authorized_code = Arc::new(MemRepository::<PreAuthorizedCodeView, Offer>::new());
    let access_token = Arc::new(MemRepository::<AccessTokenView, Offer>::new());
    let credential = Arc::new(MemRepository::default());
    let offer = Arc::new(MemRepository::default());
    let all_credentials = Arc::new(MemRepository::default());
    let all_offers = Arc::new(MemRepository::default());

    // Create custom-queries for the offer aggregate.
    let pre_authorized_code_query = PreAuthorizedCodeQuery::new(pre_authorized_code.clone());
    let access_token_query = AccessTokenQuery::new(access_token.clone());

    let all_credentials_query = ListAllQuery::new(all_credentials.clone(), "all_credentials");
    let all_offers_query = ListAllQuery::new(all_offers.clone(), "all_offers");

    // Partition the event_publishers into the different aggregates.
    let Partitions {
        server_config_event_publishers,
        credential_event_publishers,
        offer_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    IssuanceState {
        command: agent_issuance::state::CommandHandlers {
            server_config: Arc::new(
                server_config_event_publishers.into_iter().fold(
                    AggregateHandler::new(())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(server_config.clone())),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            credential: Arc::new(
                credential_event_publishers.into_iter().fold(
                    AggregateHandler::new(issuance_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(credential.clone()))
                        .append_query(all_credentials_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            offer: Arc::new(
                offer_event_publishers.into_iter().fold(
                    AggregateHandler::new(issuance_services)
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(offer.clone()))
                        .append_query(all_offers_query)
                        .append_query(pre_authorized_code_query)
                        .append_query(access_token_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
        },
        query: agent_issuance::state::ViewRepositories {
            server_config,
            pre_authorized_code,
            access_token,
            credential,
            all_credentials,
            offer,
            all_offers,
        },
    }
}

pub async fn holder_state(
    holder_services: Arc<HolderServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> HolderState {
    // Initialize the in-memory repositories.
    let holder_credential = Arc::new(MemRepository::default());
    let all_holder_credentials = Arc::new(MemRepository::default());
    let presentation = Arc::new(MemRepository::default());
    let all_presentations = Arc::new(MemRepository::default());
    let received_offer = Arc::new(MemRepository::default());
    let all_received_offers = Arc::new(MemRepository::default());

    // Create custom-queries for the offer aggregate.
    let all_holder_credentials_query = ListAllQuery::new(all_holder_credentials.clone(), "all_holder_credentials");
    let all_presentations_query = ListAllQuery::new(all_presentations.clone(), "all_presentations");
    let all_received_offers_query = ListAllQuery::new(all_received_offers.clone(), "all_received_offers");

    // Partition the event_publishers into the different aggregates.
    let Partitions {
        holder_credential_event_publishers,
        presentation_event_publishers,
        received_offer_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    HolderState {
        command: agent_holder::state::CommandHandlers {
            credential: Arc::new(
                holder_credential_event_publishers.into_iter().fold(
                    AggregateHandler::new(holder_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(holder_credential.clone()))
                        .append_query(all_holder_credentials_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            presentation: Arc::new(
                presentation_event_publishers.into_iter().fold(
                    AggregateHandler::new(holder_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(presentation.clone()))
                        .append_query(all_presentations_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            offer: Arc::new(
                received_offer_event_publishers.into_iter().fold(
                    AggregateHandler::new(holder_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(received_offer.clone()))
                        .append_query(all_received_offers_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
        },
        query: agent_holder::state::ViewRepositories {
            holder_credential,
            all_holder_credentials,
            presentation,
            all_presentations,
            received_offer,
            all_received_offers,
        },
    }
}

pub async fn verification_state(
    verification_services: Arc<VerificationServices>,
    event_publishers: Vec<Box<dyn EventPublisher>>,
) -> VerificationState {
    // Initialize the in-memory repositories.
    let authorization_request = Arc::new(MemRepository::default());
    let all_authorization_requests = Arc::new(MemRepository::default());
    let connection = Arc::new(MemRepository::default());

    // Create custom-queries for the offer aggregate.
    let all_authorization_requests_query =
        ListAllQuery::new(all_authorization_requests.clone(), "all_authorization_requests");

    // Partition the event_publishers into the different aggregates.
    let Partitions {
        authorization_request_event_publishers,
        connection_event_publishers,
        ..
    } = partition_event_publishers(event_publishers);

    VerificationState {
        command: agent_verification::state::CommandHandlers {
            authorization_request: Arc::new(
                authorization_request_event_publishers.into_iter().fold(
                    AggregateHandler::new(verification_services.clone())
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(authorization_request.clone()))
                        .append_query(all_authorization_requests_query),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
            connection: Arc::new(
                connection_event_publishers.into_iter().fold(
                    AggregateHandler::new(verification_services)
                        .append_query(SimpleLoggingQuery {})
                        .append_query(generic_query(connection.clone())),
                    |aggregate_handler, event_publisher| aggregate_handler.append_event_publisher(event_publisher),
                ),
            ),
        },
        query: agent_verification::state::ViewRepositories {
            authorization_request,
            all_authorization_requests,
            connection,
        },
    }
}
