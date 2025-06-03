use crate::{AggregateHandler, EventStoreTemp};
use agent_shared::application_state::Command;
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
