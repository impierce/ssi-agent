use agent_issuance::state::CQRS;
use async_trait::async_trait;
use cqrs_es::mem_store::MemStore;
use cqrs_es::persist::{GenericQuery, PersistenceError, ViewContext, ViewRepository};
use cqrs_es::CqrsFramework;
use cqrs_es::{Aggregate, Query, View};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct MemRepository<V: View<A>, A: Aggregate> {
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
            .unwrap()
            .get(view_id)
            .map(|view| serde_json::from_value(view.clone()).unwrap()))
    }

    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError> {
        Ok(self.map.lock().unwrap().get(view_id).map(|view| {
            let view = serde_json::from_value(view.clone()).unwrap();
            let view_context = ViewContext::new(view_id.to_string(), 0);
            (view, view_context)
        }))
    }

    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError> {
        let payload = serde_json::to_value(&view).unwrap();
        self.map.lock().unwrap().insert(context.view_instance_id, payload);
        Ok(())
    }
}

#[derive(Clone)]
pub struct ApplicationState<A: Aggregate, V: View<A>> {
    pub cqrs: Arc<CqrsFramework<A, MemStore<A>>>,
    pub issuance_data_query: Arc<MemRepository<V, A>>,
}

impl<A, V> ApplicationState<A, V>
where
    A: Aggregate + 'static,
    V: View<A> + 'static,
{
}

#[async_trait]
impl<A: Aggregate + 'static, V: View<A> + 'static> CQRS<A, V> for ApplicationState<A, V> {
    async fn new(
        queries: Vec<Box<dyn Query<A>>>,
        services: A::Services,
    ) -> agent_issuance::state::ApplicationState<A, V>
    where
        Self: Sized,
    {
        let credential_view_repo = Arc::new(MemRepository::<V, A>::new());
        let mut issuance_data_query = GenericQuery::new(credential_view_repo.clone());
        issuance_data_query.use_error_handler(Box::new(|e| println!("{}", e)));

        let mut queries = queries;
        queries.push(Box::new(issuance_data_query));

        Arc::new(ApplicationState {
            cqrs: Arc::new(CqrsFramework::new(MemStore::default(), queries, services)),
            issuance_data_query: credential_view_repo,
        }) as agent_issuance::state::ApplicationState<A, V>
    }
    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: A::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<A::Error>>
    where
        A::Command: Send + Sync,
    {
        self.cqrs.execute_with_metadata(aggregate_id, command, metadata).await
    }

    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
        self.issuance_data_query.load(view_id).await
    }
}
