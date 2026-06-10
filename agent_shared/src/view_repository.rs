use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext, ViewRepository as CoreViewRepository},
    Aggregate, View,
};

/// Dyn-compatible wrapper for cqrs-es view repositories.
///
/// cqrs-es 0.5 made `ViewRepository` non-object-safe by returning `impl Future`.
/// This trait provides object-safe async methods that can be used behind `dyn`.
#[async_trait]
pub trait DynViewRepository<V, A>: Send + Sync {
    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError>;
    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError>;
    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError>;
}

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