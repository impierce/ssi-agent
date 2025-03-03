use crate::{partition_event_publishers, AggregateHandler, EventPublisher, EventStoreTemp, Partitions};
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

impl<A> AggregateHandler<A, MemStore<A>>
where
    A: Aggregate,
    <A as Aggregate>::Command: Send,
{
    fn new(services: A::Services) -> Self {
        Self {
            cqrs: CqrsFramework::new(MemStore::default(), vec![], services),
        }
    }
}

pub struct InMemory;

impl EventStoreTemp for InMemory {
    async fn commands_and_queries<V: View<A> + 'static, A: Aggregate + 'static, AV: View<A> + 'static>(
        services: A::Services,
        event_publishers: Vec<Box<dyn Query<A>>>,
    ) -> (
        Arc<dyn Command<A> + Send + Sync>,
        Arc<dyn ViewRepository<V, A>>,
        Arc<dyn ViewRepository<AV, A>>,
    )
    where
        <A as Aggregate>::Command: Send + Sync,
    {
        let all_aggregates_name = format!("all_{}s", A::aggregate_type());

        // Initialize the in-memory repositories.
        let aggregate: Arc<MemRepository<V, A>> = Arc::new(MemRepository::default());
        let all_aggregates: Arc<MemRepository<AV, A>> = Arc::new(MemRepository::default());

        (
            Arc::new(AggregateHandler::new(services).with_parameters(
                aggregate.clone(),
                all_aggregates.clone(),
                event_publishers,
                &all_aggregates_name,
            )),
            aggregate,
            all_aggregates,
        )
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
