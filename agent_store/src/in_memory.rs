use crate::{AggregateHandler, CqrsComponentBuilder};
use agent_shared::application_state::Command;
use async_trait::async_trait;
use cqrs_es::{
    mem_store::MemStore,
    persist::{PersistenceError, ViewContext, ViewRepository},
    Aggregate, CqrsFramework, EventEnvelope, Query, View,
};
use shared_kernel::view_repository::DynViewRepository;
use std::{collections::HashMap, marker::PhantomData, sync::Arc};
use tokio::sync::RwLock;

/// Views held in memory, typed and versioned.
///
/// Projections update a view in place under the write lock ([`InMemoryViewRepository::modify`]), so
/// concurrent commands cannot overwrite each other's changes to a shared view (such as an
/// `all_*` view). [`ViewRepository::update_view`] keeps optimistic locking for other writers.
pub(crate) struct InMemoryViewRepository<V: View<A>, A: Aggregate> {
    views: RwLock<HashMap<String, (V, i64)>>,
    _phantom: PhantomData<A>,
}

impl<V: View<A>, A: Aggregate> Default for InMemoryViewRepository<V, A> {
    fn default() -> Self {
        Self {
            views: RwLock::new(HashMap::new()),
            _phantom: PhantomData,
        }
    }
}

impl<V: View<A>, A: Aggregate> InMemoryViewRepository<V, A> {
    /// Applies `update` to the view, starting from its default when it does not exist yet.
    pub(crate) async fn modify(&self, view_id: &str, update: impl FnOnce(&mut V)) {
        let mut views = self.views.write().await;
        let (view, version) = views.entry(view_id.to_string()).or_insert_with(|| (V::default(), 0));
        update(view);
        *version += 1;
    }

    pub(crate) async fn apply(&self, view_id: &str, events: &[EventEnvelope<A>]) {
        self.modify(view_id, |view| {
            for event in events {
                view.update(event);
            }
        })
        .await;
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
        let current = views.get(&context.view_instance_id).map_or(0, |(_, version)| *version);
        if current != context.version {
            return Err(PersistenceError::OptimisticLockError);
        }
        views.insert(context.view_instance_id, (view, context.version + 1));
        Ok(())
    }
}

/// Updates the view of the aggregate the events belong to, in place.
pub(crate) struct InMemoryViewQuery<V: View<A>, A: Aggregate> {
    repository: Arc<InMemoryViewRepository<V, A>>,
}

impl<V: View<A>, A: Aggregate> InMemoryViewQuery<V, A> {
    pub(crate) fn new(repository: Arc<InMemoryViewRepository<V, A>>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<V: View<A> + 'static, A: Aggregate + 'static> Query<A> for InMemoryViewQuery<V, A> {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<A>]) {
        self.repository.apply(aggregate_id, events).await;
    }
}

/// Updates the view listing all aggregates of a type, in place.
pub(crate) struct InMemoryListAllQuery<V: View<A>, A: Aggregate> {
    repository: Arc<InMemoryViewRepository<V, A>>,
    view_id: String,
}

impl<V: View<A>, A: Aggregate> InMemoryListAllQuery<V, A> {
    pub(crate) fn new(repository: Arc<InMemoryViewRepository<V, A>>, view_id: &str) -> Self {
        Self {
            repository,
            view_id: view_id.to_string(),
        }
    }
}

#[async_trait]
impl<V: View<A> + 'static, A: Aggregate + 'static> Query<A> for InMemoryListAllQuery<V, A> {
    async fn dispatch(&self, _aggregate_id: &str, events: &[EventEnvelope<A>]) {
        self.repository.apply(&self.view_id, events).await;
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
    async fn commands_and_queries<V: View<A> + Clone + 'static, A: Aggregate + 'static, AV: View<A> + Clone + 'static>(
        &self,
        services: A::Services,
        queries: Vec<Box<dyn Query<A>>>,
    ) -> (
        Arc<dyn Command<A> + Send + Sync>,
        Arc<dyn DynViewRepository<V, A>>,
        Arc<dyn DynViewRepository<AV, A>>,
    )
    where
        <A as Aggregate>::Command: Send + Sync,
    {
        let all_aggregates_name = format!("all_{}s", A::TYPE);

        let aggregate: Arc<InMemoryViewRepository<V, A>> = Arc::new(InMemoryViewRepository::default());
        let all_aggregates: Arc<InMemoryViewRepository<AV, A>> = Arc::new(InMemoryViewRepository::default());

        (
            Arc::new(AggregateHandler::new(services).with_in_memory_views(
                aggregate.clone(),
                all_aggregates.clone(),
                queries,
                &all_aggregates_name,
            )),
            aggregate,
            all_aggregates,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::{event_sink::EventSink, DomainEvent};
    use serde::{Deserialize, Serialize};
    use std::convert::Infallible;

    #[derive(Default, Deserialize, Serialize)]
    struct TestAggregate;

    #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
    struct Added(usize);

    impl DomainEvent for Added {
        fn event_type(&self) -> String {
            "Added".to_string()
        }

        fn event_version(&self) -> String {
            "1".to_string()
        }
    }

    impl Aggregate for TestAggregate {
        const TYPE: &'static str = "in_memory_test";
        type Command = Infallible;
        type Event = Added;
        type Error = Infallible;
        type Services = ();

        async fn handle(
            &mut self,
            command: Self::Command,
            _: &Self::Services,
            _: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            match command {}
        }

        fn apply(&mut self, _event: Self::Event) {}
    }

    #[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
    struct Values(Vec<usize>);

    impl View<TestAggregate> for Values {
        fn update(&mut self, event: &EventEnvelope<TestAggregate>) {
            self.0.push(event.payload.0);
        }
    }

    fn event(aggregate_id: &str, value: usize) -> EventEnvelope<TestAggregate> {
        EventEnvelope {
            aggregate_id: aggregate_id.to_string(),
            sequence: 1,
            payload: Added(value),
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn concurrent_updates_of_a_shared_view_are_all_kept() {
        let repository = Arc::new(InMemoryViewRepository::<Values, TestAggregate>::default());
        let query = Arc::new(InMemoryListAllQuery::new(repository.clone(), "all"));

        let updates = (0..100).map(|value| {
            let query = query.clone();
            tokio::spawn(async move { query.dispatch(&format!("id-{value}"), &[event("id", value)]).await })
        });
        futures::future::join_all(updates).await;

        let mut values = ViewRepository::load(&*repository, "all").await.unwrap().unwrap().0;
        values.sort_unstable();
        assert_eq!(values, (0..100).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn a_stale_view_is_not_written() {
        let repository = InMemoryViewRepository::<Values, TestAggregate>::default();
        repository.apply("id", &[event("id", 1)]).await;
        let (view, context) = ViewRepository::load_with_context(&repository, "id")
            .await
            .unwrap()
            .unwrap();
        repository.apply("id", &[event("id", 2)]).await;

        assert!(matches!(
            ViewRepository::update_view(&repository, view, context).await,
            Err(PersistenceError::OptimisticLockError)
        ));
        assert_eq!(
            ViewRepository::load(&repository, "id").await.unwrap().unwrap(),
            Values(vec![1, 2])
        );
    }
}
