use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext, ViewRepository},
    Aggregate, EventEnvelope, Query, View,
};
use std::marker::PhantomData;
use std::sync::Arc;
use tracing::{debug, error};

const MAX_OPTIMISTIC_LOCK_RETRIES: usize = 5;

/// Extension trait for queries that need mutable access to views.
///
/// While [`GenericQuery`](cqrs_es::persist::GenericQuery) handles the common "load-update-save" cycle automatically,
/// some queries (like [`ListAllQuery`]) need custom load/update logic — for example,
/// initialising a default view when none exists yet.
#[async_trait]
pub trait CustomQuery<R, V, A>: Query<A>
where
    R: ViewRepository<V, A>,
    V: View<A>,
    A: Aggregate,
{
    /// Load a view for mutation, creating a default if it doesn't exist.
    async fn load_mut(&self, view_id: String) -> Result<(V, ViewContext), PersistenceError>;

    /// Apply a batch of events to the view identified by `view_id`.
    async fn apply_events(&self, view_id: &str, events: &[EventEnvelope<A>]) -> Result<(), PersistenceError>;
}

/// A query that accumulates all events for an aggregate type into a single shared view.
///
/// Unlike [`GenericQuery`](cqrs_es::persist::GenericQuery) which maintains one view per aggregate instance, `ListAllQuery`
/// routes **all** events (regardless of aggregate ID) into one view identified by a fixed key.
/// This makes it useful for "list all" / overview projections.
pub struct ListAllQuery<R, V, A>
where
    R: ViewRepository<V, A>,
    V: View<A>,
    A: Aggregate,
{
    view_id: String,
    view_repository: Arc<R>,
    _phantom: PhantomData<(V, A)>,
}

impl<R, V, A> ListAllQuery<R, V, A>
where
    R: ViewRepository<V, A>,
    V: View<A>,
    A: Aggregate,
{
    pub fn new(view_repository: Arc<R>, view_id: &str) -> Self {
        ListAllQuery {
            view_id: view_id.to_string(),
            view_repository,
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<R, V, A> Query<A> for ListAllQuery<R, V, A>
where
    R: ViewRepository<V, A>,
    V: View<A>,
    A: Aggregate,
{
    async fn dispatch(&self, _view_id: &str, events: &[EventEnvelope<A>]) {
        debug!(view_id = %self.view_id, event_count = events.len(), "ListAllQuery dispatching events");
        if let Err(err) = self.apply_events(&self.view_id, events).await {
            error!(
                view_id = %self.view_id,
                event_count = events.len(),
                error = %err,
                "ListAllQuery failed to apply events to list view"
            );
        }
    }
}

#[async_trait]
impl<R, V, A> CustomQuery<R, V, A> for ListAllQuery<R, V, A>
where
    R: ViewRepository<V, A>,
    V: View<A>,
    A: Aggregate,
{
    async fn load_mut(&self, view_id: String) -> Result<(V, ViewContext), PersistenceError> {
        match self.view_repository.load_with_context(&view_id).await? {
            None => {
                let view_context = ViewContext::new(view_id, 0);
                Ok((Default::default(), view_context))
            }
            Some((view, context)) => Ok((view, context)),
        }
    }

    async fn apply_events(&self, view_id: &str, events: &[EventEnvelope<A>]) -> Result<(), PersistenceError> {
        if events.is_empty() {
            return Ok(());
        }

        for attempt in 0..=MAX_OPTIMISTIC_LOCK_RETRIES {
            let (mut view, view_context) = self.load_mut(view_id.to_string()).await?;
            for event in events {
                view.update(event);
            }

            match self.view_repository.update_view(view, view_context).await {
                Ok(()) => {
                    debug!(
                        view_id,
                        event_count = events.len(),
                        attempt,
                        "ListAllQuery applied events to list view"
                    );
                    return Ok(());
                }
                Err(PersistenceError::OptimisticLockError) if attempt < MAX_OPTIMISTIC_LOCK_RETRIES => {
                    debug!(
                        view_id,
                        attempt = attempt + 1,
                        max_retries = MAX_OPTIMISTIC_LOCK_RETRIES,
                        "ListAllQuery encountered optimistic lock conflict; retrying"
                    );
                    tokio::task::yield_now().await;
                }
                Err(err) => return Err(err),
            }
        }

        unreachable!("retry loop returns on success or final error")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::in_memory::MemViewRepository;
    use cqrs_es::{event_sink::EventSink, DomainEvent};
    use serde::{Deserialize, Serialize};
    use std::{collections::HashMap, slice};

    // ── Test aggregate ─────────────────────────────────────────

    #[derive(Default, Debug, Serialize, Deserialize)]
    struct TestAggregate;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestEvent;

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> String {
            "TestEvent".into()
        }
        fn event_version(&self) -> String {
            "1.0".into()
        }
    }

    #[derive(Debug)]
    struct TestError;

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "TestError")
        }
    }

    impl std::error::Error for TestError {}

    impl Aggregate for TestAggregate {
        const TYPE: &'static str = "TestAggregate";
        type Command = String;
        type Event = TestEvent;
        type Error = TestError;
        type Services = ();

        async fn handle(
            &mut self,
            _command: Self::Command,
            _services: &Self::Services,
            _sink: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn apply(&mut self, _: Self::Event) {}
    }

    // ── Counter view ──────────────────────────────────────────

    /// A view that counts how many events it has seen.
    #[derive(Default, Debug, Clone, Serialize, Deserialize)]
    struct CounterView {
        count: u32,
    }

    impl View<TestAggregate> for CounterView {
        fn update(&mut self, _event: &EventEnvelope<TestAggregate>) {
            self.count += 1;
        }
    }

    // ── Tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn list_all_query_accumulates_events() {
        let repo: Arc<MemViewRepository<CounterView, TestAggregate>> = Arc::new(MemViewRepository::default());
        let query = ListAllQuery::<_, CounterView, TestAggregate>::new(repo.clone(), "all-items");

        let event = EventEnvelope {
            aggregate_id: "aggregate-id".to_string(),
            sequence: 1,
            payload: TestEvent,
            metadata: HashMap::new(),
        };

        Query::dispatch(&query, "aggregate-id", slice::from_ref(&event)).await;
        Query::dispatch(&query, "aggregate-id", slice::from_ref(&event)).await;
        Query::dispatch(&query, "aggregate-id", &[event]).await;

        let view = ViewRepository::load(repo.as_ref(), "all-items").await.unwrap().unwrap();
        assert_eq!(view.count, 3);
    }

    #[tokio::test]
    async fn list_all_query_creates_default_view_on_first_event() {
        let repo: Arc<MemViewRepository<CounterView, TestAggregate>> = Arc::new(MemViewRepository::default());
        let query = ListAllQuery::<_, CounterView, TestAggregate>::new(repo.clone(), "list");

        // No view exists yet — load_mut should initialise a default.
        let (view, ctx) = CustomQuery::load_mut(&query, "list".to_string()).await.unwrap();
        assert_eq!(view.count, 0);
        assert_eq!(ctx.view_instance_id, "list");
    }

    #[derive(Default)]
    struct CountingRepo {
        inner: MemViewRepository<CounterView, TestAggregate>,
        loads: std::sync::atomic::AtomicUsize,
        updates: std::sync::atomic::AtomicUsize,
        conflicts_remaining: std::sync::Mutex<usize>,
        fail_with_unknown_error: std::sync::atomic::AtomicBool,
    }

    impl ViewRepository<CounterView, TestAggregate> for CountingRepo {
        async fn load(&self, view_id: &str) -> Result<Option<CounterView>, PersistenceError> {
            ViewRepository::load(&self.inner, view_id).await
        }

        async fn load_with_context(
            &self,
            view_id: &str,
        ) -> Result<Option<(CounterView, ViewContext)>, PersistenceError> {
            self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ViewRepository::load_with_context(&self.inner, view_id).await
        }

        async fn update_view(&self, view: CounterView, context: ViewContext) -> Result<(), PersistenceError> {
            self.updates.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail_with_unknown_error.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(PersistenceError::UnknownError(Box::new(std::io::Error::other("boom"))));
            }
            {
                let mut remaining = self.conflicts_remaining.lock().unwrap();
                if *remaining > 0 {
                    *remaining -= 1;
                    return Err(PersistenceError::OptimisticLockError);
                }
            }
            ViewRepository::update_view(&self.inner, view, context).await
        }
    }

    #[tokio::test]
    async fn list_all_query_batches_events_and_skips_empty_batches() {
        let repo = Arc::new(CountingRepo::default());
        let query = ListAllQuery::<_, CounterView, TestAggregate>::new(repo.clone(), "list");

        query.apply_events("list", &[]).await.unwrap();
        assert_eq!(repo.loads.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(repo.updates.load(std::sync::atomic::Ordering::SeqCst), 0);

        let batch: Vec<_> = (1..=3)
            .map(|sequence| EventEnvelope {
                aggregate_id: "agg-1".to_string(),
                sequence,
                payload: TestEvent,
                metadata: HashMap::new(),
            })
            .collect();

        query.apply_events("list", &batch).await.unwrap();
        assert_eq!(repo.loads.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(repo.updates.load(std::sync::atomic::Ordering::SeqCst), 1);

        let view = ViewRepository::load(repo.as_ref(), "list").await.unwrap().unwrap();
        assert_eq!(view.count, 3);
    }

    #[tokio::test]
    async fn list_all_query_retries_on_optimistic_lock_error_and_does_not_retry_other_errors() {
        let repo = Arc::new(CountingRepo::default());
        *repo.conflicts_remaining.lock().unwrap() = 2;
        let query = ListAllQuery::<_, CounterView, TestAggregate>::new(repo.clone(), "list");

        let batch = vec![
            EventEnvelope {
                aggregate_id: "agg-1".to_string(),
                sequence: 1,
                payload: TestEvent,
                metadata: HashMap::new(),
            },
            EventEnvelope {
                aggregate_id: "agg-1".to_string(),
                sequence: 2,
                payload: TestEvent,
                metadata: HashMap::new(),
            },
        ];

        query.apply_events("list", &batch).await.unwrap();
        assert_eq!(repo.loads.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert_eq!(repo.updates.load(std::sync::atomic::Ordering::SeqCst), 3);
        let view = ViewRepository::load(repo.as_ref(), "list").await.unwrap().unwrap();
        assert_eq!(view.count, 2);

        // Exhausting retries returns OptimisticLockError
        repo.loads.store(0, std::sync::atomic::Ordering::SeqCst);
        repo.updates.store(0, std::sync::atomic::Ordering::SeqCst);
        *repo.conflicts_remaining.lock().unwrap() = MAX_OPTIMISTIC_LOCK_RETRIES + 5;
        let err = query.apply_events("list", &batch).await.unwrap_err();
        assert!(matches!(err, PersistenceError::OptimisticLockError));
        assert_eq!(
            repo.updates.load(std::sync::atomic::Ordering::SeqCst),
            MAX_OPTIMISTIC_LOCK_RETRIES + 1
        );

        // Non-optimistic errors fail immediately without retrying
        repo.updates.store(0, std::sync::atomic::Ordering::SeqCst);
        repo.fail_with_unknown_error
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let err = query.apply_events("list", &batch).await.unwrap_err();
        assert!(matches!(err, PersistenceError::UnknownError(_)));
        assert_eq!(repo.updates.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
