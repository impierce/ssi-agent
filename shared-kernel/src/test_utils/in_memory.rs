use crate::command_handler::{CommandHandler, CommandHandlerFactory};
use crate::view_repository::{BoxedViewRepository, ViewRepositoryFactory};
use cqrs_es::mem_store::MemStore;
use cqrs_es::CqrsFramework;
use cqrs_es::{
    persist::{PersistenceError, ViewContext, ViewRepository as CoreViewRepository},
    Aggregate, Query, View,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

/// An in-memory store backend that implements [`ViewRepositoryFactory`] and [`CommandHandlerFactory`].
///
/// Useful for unit and integration tests where persistence is not needed.
pub struct InMemoryStore;

impl ViewRepositoryFactory for InMemoryStore {
    fn create_view_repository<V, A>(&self, _name: &str) -> BoxedViewRepository<V, A>
    where
        V: View<A> + Clone + 'static,
        A: Aggregate + 'static,
    {
        BoxedViewRepository::new(Box::new(MemViewRepository::default()))
    }
}

impl CommandHandlerFactory for InMemoryStore {
    type Error = std::convert::Infallible;

    async fn create_handler<A>(
        &self,
        services: A::Services,
        queries: Vec<Box<dyn Query<A>>>,
    ) -> Result<CommandHandler<A>, Self::Error>
    where
        A: Aggregate + 'static,
        <A as Aggregate>::Command: Send,
    {
        Ok(Arc::new(CqrsFramework::new(MemStore::default(), queries, services)) as CommandHandler<A>)
    }
}

/// A simple in-memory [`ViewRepository`](CoreViewRepository) backed by a `HashMap`.
///
/// Clones views on read. Suitable for testing only.
#[derive(Default)]
pub struct MemViewRepository<V: View<A> + Clone, A: Aggregate> {
    map: Mutex<HashMap<String, V>>,
    _phantom: std::marker::PhantomData<A>,
}

impl<V, A> CoreViewRepository<V, A> for MemViewRepository<V, A>
where
    V: View<A> + Clone,
    A: Aggregate,
{
    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
        Ok(self.map.lock().await.get(view_id).cloned())
    }

    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError> {
        Ok(self
            .map
            .lock()
            .await
            .get(view_id)
            .map(|view| (view.clone(), ViewContext::new(view_id.to_string(), 0))))
    }

    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError> {
        self.map.lock().await.insert(context.view_instance_id, view);
        Ok(())
    }
}
