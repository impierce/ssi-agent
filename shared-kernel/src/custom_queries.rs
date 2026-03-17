use async_trait::async_trait;
use cqrs_es::{
    persist::{GenericQuery, PersistenceError, ViewContext, ViewRepository},
    Aggregate, EventEnvelope, Query, View,
};
use std::marker::PhantomData;
use std::sync::Arc;
use tracing::debug;

/// Create the standard pair of queries for an aggregate: a [`GenericQuery`] for the
/// single-entity view and a [`ListAllQuery`] for the list view.
///
/// Returns a `Vec<Box<dyn Query<A>>>` containing exactly two queries:
/// 1. A `GenericQuery` that projects individual aggregate events into the single view.
/// 2. A `ListAllQuery` that accumulates all aggregate events into a shared list view
///    identified by `list_key`.
///
/// This eliminates the repetitive query wiring that every bounded context builder
/// would otherwise duplicate per aggregate.
pub fn standard_queries<V, LV, R1, R2, A>(
    single_view: &Arc<R1>,
    list_view: &Arc<R2>,
    list_key: &str,
) -> Vec<Box<dyn Query<A>>>
where
    V: View<A> + 'static,
    LV: View<A> + 'static,
    R1: ViewRepository<V, A> + 'static,
    R2: ViewRepository<LV, A> + 'static,
    A: Aggregate + 'static,
{
    vec![
        Box::new(GenericQuery::new(single_view.clone())),
        Box::new(ListAllQuery::new(list_view.clone(), list_key)),
    ]
}

/// Extension trait for queries that need mutable access to views.
///
/// While [`GenericQuery`] handles the common "load-update-save" cycle automatically,
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
/// Unlike [`GenericQuery`] which maintains one view per aggregate instance, `ListAllQuery`
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
        self.apply_events(&self.view_id, events).await.ok();
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
        for event in events {
            let (mut view, view_context) = self.load_mut(view_id.to_string()).await?;

            view.update(event);
            self.view_repository.update_view(view, view_context).await?;
        }
        debug!(
            view_id,
            event_count = events.len(),
            "ListAllQuery applied events to list view"
        );
        Ok(())
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
}
