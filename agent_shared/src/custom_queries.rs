use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext, ViewRepository},
    Aggregate, EventEnvelope, Query, View,
};
use std::marker::PhantomData;
use std::sync::Arc;

/// A custom query trait. This trait is used to define custom queries for the Aggregates that do not make use of
/// `GenericQuery`.
#[async_trait]
pub trait CustomQuery<R, V, A>: Query<A>
where
    R: ViewRepository<V, A>,
    V: View<A>,
    A: Aggregate,
{
    async fn load_mut(&self, view_id: String) -> Result<(V, ViewContext), PersistenceError>;

    async fn apply_events(&self, view_id: &str, events: &[EventEnvelope<A>]) -> Result<(), PersistenceError>;
}

/// A struct that lists all the instances of an `Aggregate`.
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
        Ok(())
    }
}
