use crate::{custom_queries::MutableViewRepository, AggregateHandler, CqrsComponentBuilder};
use agent_shared::application_state::Command;
use cqrs_es::{
    mem_store::MemStore,
    persist::{PersistenceError, ViewContext, ViewRepository},
    Aggregate, CqrsFramework, Query, View,
};
use shared_kernel::view_repository::DynViewRepository;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

#[async_trait::async_trait]
impl<V, A> MutableViewRepository<V, A> for InMemoryViewRepository<V, A>
where
    V: View<A> + Clone,
    A: Aggregate,
{
    async fn modify(
        &self,
        view_id: &str,
        update: &mut (dyn for<'view> FnMut(&'view mut V) + Send),
    ) -> Result<(), PersistenceError> {
        let mut views = self.views.write().await;
        let (view, version) = views.entry(view_id.to_string()).or_insert_with(|| (V::default(), 0));
        update(view);
        *version += 1;
        Ok(())
    }
}

pub(crate) struct InMemoryViewRepository<V: View<A>, A: Aggregate> {
    views: RwLock<HashMap<String, (V, i64)>>,
    _phantom: std::marker::PhantomData<(V, A)>,
}

impl<V, A> Default for InMemoryViewRepository<V, A>
where
    V: View<A>,
    A: Aggregate,
{
    fn default() -> Self {
        Self {
            views: RwLock::new(HashMap::new()),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<V, A> ViewRepository<V, A> for InMemoryViewRepository<V, A>
where
    V: View<A> + Clone,
    A: Aggregate,
{
    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
        Ok(self.views.read().await.get(view_id).map(|(view, _)| view.clone()))
    }

    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError> {
        Ok(self
            .views
            .read()
            .await
            .get(view_id)
            .map(|(view, version)| (view.clone(), ViewContext::new(view_id.to_string(), *version))))
    }

    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError> {
        let mut views = self.views.write().await;
        match views.get(&context.view_instance_id) {
            Some((_, version)) if *version != context.version => {
                return Err(PersistenceError::OptimisticLockError);
            }
            None if context.version != 0 => return Err(PersistenceError::OptimisticLockError),
            _ => {}
        }
        views.insert(context.view_instance_id, (view, context.version + 1));
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
            execution: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

pub struct InMemory;

impl CqrsComponentBuilder for InMemory {
    async fn commands_and_queries<V: View<A> + Clone + 'static, A: Aggregate + 'static, AV: View<A> + Clone + 'static>(
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
        let aggregate: Arc<InMemoryViewRepository<V, A>> = Arc::new(InMemoryViewRepository::default());
        let all_aggregates: Arc<InMemoryViewRepository<AV, A>> = Arc::new(InMemoryViewRepository::default());

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
