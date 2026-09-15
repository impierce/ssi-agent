use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext, ViewRepository},
    Aggregate, EventEnvelope, Query, View,
};
use std::{marker::PhantomData, sync::Arc};
use tracing::error;

#[async_trait]
pub trait MutableViewRepository<V, A>: ViewRepository<V, A> + Send + Sync
where
    V: View<A>,
    A: Aggregate,
{
    async fn modify(
        &self,
        view_id: &str,
        update: &mut (dyn for<'view> FnMut(&'view mut V) + Send),
    ) -> Result<(), PersistenceError>;
}

pub(crate) async fn modify_via_store<R, V, A>(
    repository: &R,
    view_id: &str,
    update: &mut (dyn for<'view> FnMut(&'view mut V) + Send),
) -> Result<(), PersistenceError>
where
    R: ViewRepository<V, A>,
    V: View<A>,
    A: Aggregate,
{
    const MAX_OPTIMISTIC_LOCK_RETRIES: usize = 8;

    for attempt in 0..=MAX_OPTIMISTIC_LOCK_RETRIES {
        let (mut view, context) = repository
            .load_with_context(view_id)
            .await?
            .unwrap_or_else(|| (V::default(), ViewContext::new(view_id.to_string(), 0)));
        update(&mut view);
        match repository.update_view(view, context).await {
            Err(PersistenceError::OptimisticLockError) if attempt < MAX_OPTIMISTIC_LOCK_RETRIES => continue,
            result => return result,
        }
    }

    unreachable!("bounded optimistic-lock retry loop always returns")
}

pub struct MutableQuery<R, V, A>
where
    R: MutableViewRepository<V, A>,
    V: View<A>,
    A: Aggregate,
{
    view_repository: Arc<R>,
    _phantom: PhantomData<(V, A)>,
}

impl<R, V, A> MutableQuery<R, V, A>
where
    R: MutableViewRepository<V, A>,
    V: View<A>,
    A: Aggregate,
{
    pub fn new(view_repository: Arc<R>) -> Self {
        Self {
            view_repository,
            _phantom: PhantomData,
        }
    }

    async fn apply_events(&self, view_id: &str, events: &[EventEnvelope<A>]) -> Result<(), PersistenceError> {
        self.view_repository
            .modify(view_id, &mut |view| {
                for event in events {
                    view.update(event);
                }
            })
            .await
    }
}

#[async_trait]
impl<R, V, A> Query<A> for MutableQuery<R, V, A>
where
    R: MutableViewRepository<V, A> + 'static,
    V: View<A> + 'static,
    A: Aggregate + 'static,
{
    async fn dispatch(&self, view_id: &str, events: &[EventEnvelope<A>]) {
        if let Err(error) = self.apply_events(view_id, events).await {
            error!(view_id, %error, "Failed to update aggregate projection");
        }
    }
}

pub struct ListAllQuery<R, V, A>
where
    R: MutableViewRepository<V, A>,
    V: View<A>,
    A: Aggregate,
{
    view_id: String,
    view_repository: Arc<R>,
    _phantom: PhantomData<(V, A)>,
}

impl<R, V, A> ListAllQuery<R, V, A>
where
    R: MutableViewRepository<V, A>,
    V: View<A>,
    A: Aggregate,
{
    pub fn new(view_repository: Arc<R>, view_id: &str) -> Self {
        Self {
            view_id: view_id.to_string(),
            view_repository,
            _phantom: PhantomData,
        }
    }

    async fn apply_events(&self, events: &[EventEnvelope<A>]) -> Result<(), PersistenceError> {
        self.view_repository
            .modify(&self.view_id, &mut |view| {
                for event in events {
                    view.update(event);
                }
            })
            .await
    }
}

#[async_trait]
impl<R, V, A> Query<A> for ListAllQuery<R, V, A>
where
    R: MutableViewRepository<V, A> + 'static,
    V: View<A> + 'static,
    A: Aggregate + 'static,
{
    async fn dispatch(&self, _view_id: &str, events: &[EventEnvelope<A>]) {
        if let Err(error) = self.apply_events(events).await {
            error!(view_id = %self.view_id, %error, "Failed to update list projection");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::DomainEvent;
    use serde::{Deserialize, Serialize};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex;

    #[derive(Default, Serialize, Deserialize)]
    struct TestAggregate;

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct TestEvent;

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> String {
            "test".to_string()
        }

        fn event_version(&self) -> String {
            "1".to_string()
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("test error")]
    struct TestError;

    impl Aggregate for TestAggregate {
        const TYPE: &'static str = "test";
        type Command = ();
        type Event = TestEvent;
        type Error = TestError;
        type Services = ();

        async fn handle(
            &mut self,
            _command: Self::Command,
            _service: &Self::Services,
            _sink: &cqrs_es::event_sink::EventSink<Self>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn apply(&mut self, _event: Self::Event) {}
    }

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    struct CountView(usize);

    impl View<TestAggregate> for CountView {
        fn update(&mut self, _event: &EventEnvelope<TestAggregate>) {
            self.0 += 1;
        }
    }

    #[derive(Default)]
    struct CountingRepository {
        view: Mutex<Option<CountView>>,
        conflicts: AtomicUsize,
        loads: AtomicUsize,
        stores: AtomicUsize,
    }

    impl ViewRepository<CountView, TestAggregate> for CountingRepository {
        async fn load(&self, _view_id: &str) -> Result<Option<CountView>, PersistenceError> {
            self.loads.fetch_add(1, Ordering::Relaxed);
            Ok(self.view.lock().await.clone())
        }

        async fn load_with_context(&self, view_id: &str) -> Result<Option<(CountView, ViewContext)>, PersistenceError> {
            self.loads.fetch_add(1, Ordering::Relaxed);
            Ok(self
                .view
                .lock()
                .await
                .clone()
                .map(|view| (view, ViewContext::new(view_id.to_string(), 0))))
        }

        async fn update_view(&self, view: CountView, _context: ViewContext) -> Result<(), PersistenceError> {
            self.stores.fetch_add(1, Ordering::Relaxed);
            if self
                .conflicts
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(PersistenceError::OptimisticLockError);
            }
            *self.view.lock().await = Some(view);
            Ok(())
        }
    }

    #[async_trait]
    impl MutableViewRepository<CountView, TestAggregate> for CountingRepository {
        async fn modify(
            &self,
            view_id: &str,
            update: &mut (dyn for<'view> FnMut(&'view mut CountView) + Send),
        ) -> Result<(), PersistenceError> {
            modify_via_store(self, view_id, update).await
        }
    }

    fn event(sequence: usize) -> EventEnvelope<TestAggregate> {
        EventEnvelope {
            aggregate_id: "one".to_string(),
            sequence,
            payload: TestEvent,
            metadata: Default::default(),
        }
    }

    #[tokio::test]
    async fn applies_a_batch_with_one_load_and_one_store() {
        let repository = Arc::new(CountingRepository::default());
        let query = ListAllQuery::new(repository.clone(), "all_tests");

        query.apply_events(&[event(1), event(2), event(3)]).await.unwrap();

        assert_eq!(repository.loads.load(Ordering::Relaxed), 1);
        assert_eq!(repository.stores.load(Ordering::Relaxed), 1);
        assert_eq!(repository.view.lock().await.as_ref().unwrap().0, 3);
    }

    #[tokio::test]
    async fn retries_a_concurrent_persisted_view_update() {
        let repository = Arc::new(CountingRepository {
            conflicts: AtomicUsize::new(1),
            ..Default::default()
        });
        let query = ListAllQuery::new(repository.clone(), "all_tests");

        query.apply_events(&[event(1)]).await.unwrap();

        assert_eq!(repository.loads.load(Ordering::Relaxed), 2);
        assert_eq!(repository.stores.load(Ordering::Relaxed), 2);
        assert_eq!(repository.view.lock().await.as_ref().unwrap().0, 1);
    }
}
