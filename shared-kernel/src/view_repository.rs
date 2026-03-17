use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext, ViewRepository as CoreViewRepository},
    Aggregate, View,
};
use tracing::debug;

/// A trait for views that support soft deletion.
///
/// Implement this on your view types to enable the generic
/// [`load_by_id`] query helper.
pub trait SoftDeletable {
    fn is_deleted(&self) -> bool;
}

/// # Errors
///
/// Returns `PersistenceError` if the underlying repository operation fails.
/// Returns `Ok(None)` if the view doesn't exist or is soft-deleted.
/// Returns `Ok(Some(view))` if the view exists and is not soft-deleted.
pub async fn load_by_id<V, A>(repo: &BoxedViewRepository<V, A>, id: &str) -> Result<Option<V>, PersistenceError>
where
    V: View<A> + SoftDeletable,
    A: Aggregate,
{
    match repo.0.load(id).await? {
        Some(view) if !view.is_deleted() => {
            debug!(view_id = id, "View loaded");
            Ok(Some(view))
        }
        Some(_) => {
            debug!(view_id = id, "View is soft-deleted, treating as not found");
            Ok(None)
        }
        None => Ok(None),
    }
}

/// A dyn-compatible wrapper trait for view repository operations.
///
/// Because cqrs-es's [`ViewRepository`](CoreViewRepository) returns `impl Future` (not
/// dyn-compatible), this trait provides `async_trait`-based equivalents that can be
/// used behind `Box<dyn DynViewRepository>`.
#[async_trait]
pub trait DynViewRepository<V, A>: Send + Sync {
    /// Load a view by ID, returning `None` if it doesn't exist.
    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError>;
    /// Load a view together with its persistence context (used for optimistic concurrency).
    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError>;
    /// Persist an updated view with the given context.
    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError>;
}

/// Implement `DynViewRepository` for any concrete [`ViewRepository`](CoreViewRepository).
///
/// This blanket implementation erases the concrete type, enabling the use of
/// `Box<dyn DynViewRepository>` inside [`BoxedViewRepository`].
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

/// A type-erased view repository wrapping `Box<dyn DynViewRepository>`.
///
/// Also implements cqrs-es's [`ViewRepository`](CoreViewRepository) by delegating to the
/// inner trait object, so it can be passed directly to [`GenericQuery`](cqrs_es::persist::GenericQuery)
/// and other cqrs-es query types.
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
    #[must_use]
    pub fn new(repository: Box<dyn DynViewRepository<V, A>>) -> Self {
        Self(repository)
    }
}

impl<V, A> CoreViewRepository<V, A> for BoxedViewRepository<V, A>
where
    V: View<A>,
    A: Aggregate,
{
    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
        self.0.load(view_id).await
    }

    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError> {
        self.0.load_with_context(view_id).await
    }

    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError> {
        self.0.update_view(view, context).await
    }
}

/// A factory trait for creating view repository instances backed by a specific store.
///
/// Each store backend (`MongoDB`, `InMemory`, etc.) implements this once.
/// Bounded contexts use it to construct their view repositories without coupling
/// to a specific store implementation.
pub trait ViewRepositoryFactory {
    /// Create a new [`BoxedViewRepository`] with the given collection/table `name`.
    fn create_view_repository<V, A>(&self, name: &str) -> BoxedViewRepository<V, A>
    where
        V: View<A> + Clone + 'static,
        A: Aggregate + 'static;
}
