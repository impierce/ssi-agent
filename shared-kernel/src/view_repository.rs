use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext, ViewRepository as CoreViewRepository},
    Aggregate, View,
};
use std::future::Future;

/// A trait for views that support soft deletion.
///
/// Implement this on your view types to enable the generic
/// [`load_by_id`] query helper.
pub trait SoftDeletable {
    fn is_deleted(&self) -> bool;
}

/// Load a single view by ID, returning `None` if it doesn't exist or is soft-deleted.
pub async fn load_by_id<V, A>(repo: &BoxedViewRepository<V, A>, id: &str) -> Result<Option<V>, PersistenceError>
where
    V: View<A> + SoftDeletable,
    A: Aggregate,
{
    match repo.0.load(id).await? {
        Some(view) if !view.is_deleted() => Ok(Some(view)),
        _ => Ok(None),
    }
}

/// A dyn-compatible wrapper trait with async methods.
#[async_trait]
pub trait DynViewRepository<V, A>: Send + Sync {
    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError>;
    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError>;
    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError>;
}

/// Implement `DynViewRepository` for any `CoreViewRepository`
#[async_trait]
impl<V, A, VR> DynViewRepository<V, A> for VR
where
    V: View<A> + 'static,
    A: Aggregate,
    VR: CoreViewRepository<V, A>,
{
    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
        CoreViewRepository::load(self, view_id).await
    }

    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError> {
        CoreViewRepository::load_with_context(self, view_id).await
    }

    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError> {
        CoreViewRepository::update_view(self, view, context).await
    }
}

pub struct BoxedViewRepository<V, A>(Box<dyn DynViewRepository<V, A>>)
where
    V: View<A>,
    A: Aggregate;

impl<V, A> std::ops::Deref for BoxedViewRepository<V, A>
where
    V: View<A>,
    A: Aggregate,
{
    type Target = dyn DynViewRepository<V, A>;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl<V, A> BoxedViewRepository<V, A>
where
    V: View<A>,
    A: Aggregate,
{
    pub fn new(repository: Box<dyn DynViewRepository<V, A>>) -> Self {
        Self(repository)
    }
}

impl<V, A> CoreViewRepository<V, A> for BoxedViewRepository<V, A>
where
    V: View<A>,
    A: Aggregate,
{
    fn load(&self, view_id: &str) -> impl Future<Output = Result<Option<V>, PersistenceError>> + Send {
        async move { self.0.load(view_id).await }
    }

    fn load_with_context(
        &self,
        view_id: &str,
    ) -> impl Future<Output = Result<Option<(V, ViewContext)>, PersistenceError>> + Send {
        async move { self.0.load_with_context(view_id).await }
    }

    fn update_view(&self, view: V, context: ViewContext) -> impl Future<Output = Result<(), PersistenceError>> + Send {
        async move { self.0.update_view(view, context).await }
    }
}

/// A factory trait for creating [`ViewRepository`] instances backed by a specific store.
///
/// Each store backend (MongoDB, InMemory, Postgres, etc.) implements this once.
/// Bounded contexts use it to construct their view repositories without coupling
/// to a specific store implementation.
pub trait ViewRepositoryFactory {
    fn create_view_repository<V, A>(&self, name: &str) -> BoxedViewRepository<V, A>
    where
        V: View<A> + Clone + 'static,
        A: Aggregate + 'static;
}
