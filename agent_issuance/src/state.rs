use async_trait::async_trait;
use cqrs_es::persist::PersistenceError;
use cqrs_es::{Aggregate, View};
use std::collections::HashMap;
use std::sync::Arc;

#[async_trait]
pub trait ApplicationState<A: Aggregate, V: View<A>> {
    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: A::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<A::Error>>
    where
        A::Command: Send + Sync;

    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError>;
}
pub type DynApplicationState<A, V> = Arc<dyn ApplicationState<A, V> + Send + Sync>;
