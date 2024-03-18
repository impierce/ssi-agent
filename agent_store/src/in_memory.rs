use agent_issuance::{
    credential::services::CredentialServices,
    offer::{
        aggregate::Offer,
        queries::{
            access_token::{AccessTokenQuery, AccessTokenView},
            pre_authorized_code::{PreAuthorizedCodeQuery, PreAuthorizedCodeView},
        },
        services::OfferServices,
    },
    server_config::services::ServerConfigServices,
    state::{CommandHandlers, IssuanceState, ViewRepositories},
    SimpleLoggingQuery,
};
use agent_shared::{
    application_state::{ApplicationState, Command},
    generic_query::generic_query,
};
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
}

pub async fn application_state() -> ApplicationState<IssuanceState> {
    // Initialize the in-memory repositories.
    let server_config = Arc::new(MemRepository::default());
    let credential = Arc::new(MemRepository::default());
    let offer = Arc::new(MemRepository::default());
    let pre_authorized_code = Arc::new(MemRepository::<PreAuthorizedCodeView, Offer>::new());
    let access_token = Arc::new(MemRepository::<AccessTokenView, Offer>::new());

    // Create custom-queries for the offer aggregate.
    let pre_authorized_code_query = PreAuthorizedCodeQuery::new(pre_authorized_code.clone());
    let access_token_query = AccessTokenQuery::new(access_token.clone());

    let issuance = IssuanceState {
        command: CommandHandlers {
            server_config: Arc::new(
                AggregateHandler::new(ServerConfigServices)
                    .append_query(SimpleLoggingQuery {})
                    .append_query(generic_query(server_config.clone())),
            ),
            credential: Arc::new(
                AggregateHandler::new(CredentialServices)
                    .append_query(SimpleLoggingQuery {})
                    .append_query(generic_query(credential.clone())),
            ),
            offer: Arc::new(
                AggregateHandler::new(OfferServices)
                    .append_query(SimpleLoggingQuery {})
                    .append_query(generic_query(offer.clone()))
                    .append_query(pre_authorized_code_query)
                    .append_query(access_token_query),
            ),
        },
        query: ViewRepositories {
            server_config,
            credential,
            offer,
            pre_authorized_code,
            access_token,
        },
    };

    ApplicationState { issuance }
}
