use crate::validation::EventValidationError;
use crate::{AggregateHandler, CqrsComponentBuilder};
use agent_shared::application_state::Command;
use cqrs_es::{
    mem_store::MemStore,
    persist::{EventUpcaster, PersistenceError, ViewContext, ViewRepository},
    Aggregate, CqrsFramework, Query, View,
};
use shared_kernel::view_repository::DynViewRepository;
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

impl CqrsComponentBuilder for InMemory {
    async fn commands_and_queries<V: View<A> + 'static, A: Aggregate + 'static, AV: View<A> + 'static>(
        &self,
        services: A::Services,
        event_publishers: Vec<Box<dyn Query<A>>>,
        // `MemStore` keeps events as live, already-deserialized `EventEnvelope`s in memory and
        // never round-trips them through a serialized representation, so `EventUpcaster`s (which
        // operate on the serialized JSON payload) have nothing to act on here. The parameter is
        // accepted for API symmetry with the other backends and intentionally unused; upcaster
        // behavior is instead exercised via shared-kernel's in-memory *persisted* repository tests.
        _upcasters: Vec<Box<dyn cqrs_es::persist::EventUpcaster>>,
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

    // `MemStore` keeps events as live, already-deserialized `EventEnvelope`s in memory and never
    // round-trips them through a serialized representation, so there is nothing persisted to
    // stream, upcast, or deserialize here. Always reports success with zero events validated.
    async fn validate_events<A: Aggregate + 'static>(
        &self,
        _upcasters: Vec<Box<dyn EventUpcaster>>,
    ) -> Result<u64, EventValidationError> {
        Ok(0)
    }
}
