use crate::{AggregateHandler, CqrsComponentBuilder};
use agent_shared::application_state::Command;
use agent_shared::view_repository::DynViewRepository;
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

impl<V, A> ViewRepository<V, A> for MemRepository<V, A>
where
    V: View<A>,
    A: Aggregate,
{
    fn load(&self, view_id: &str) -> impl std::future::Future<Output = Result<Option<V>, PersistenceError>> + Send {
        async move {
            Ok(self
                .map
                .lock()
                .await
                .get(view_id)
                .map(|view| serde_json::from_value(view.clone()).unwrap()))
        }
    }

    fn load_with_context(
        &self,
        view_id: &str,
    ) -> impl std::future::Future<Output = Result<Option<(V, ViewContext)>, PersistenceError>> + Send {
        async move {
            Ok(self.map.lock().await.get(view_id).map(|view| {
                let view = serde_json::from_value(view.clone()).unwrap();
                let view_context = ViewContext::new(view_id.to_string(), 0);
                (view, view_context)
            }))
        }
    }

    fn update_view(
        &self,
        view: V,
        context: ViewContext,
    ) -> impl std::future::Future<Output = Result<(), PersistenceError>> + Send {
        async move {
            let payload = serde_json::to_value(&view).unwrap();
            self.map.lock().await.insert(context.view_instance_id, payload);
            Ok(())
        }
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

impl CqrsComponentBuilder for InMemory {
    async fn commands_and_queries<V: View<A> + 'static, A: Aggregate + 'static, AV: View<A> + 'static>(
        &self,
        services: A::Services,
        event_publishers: Vec<Box<dyn Query<A>>>,
    ) -> (
        Arc<dyn Command<A> + Send + Sync>,
        Arc<dyn DynViewRepository<V, A>>,
        Arc<dyn DynViewRepository<AV, A>>,
    )
    where
        <A as Aggregate>::Command: Send + Sync,
    {
        let all_aggregates_name = format!("all_{}s", A::TYPE);

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
