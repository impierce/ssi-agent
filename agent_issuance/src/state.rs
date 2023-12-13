use async_trait::async_trait;
use cqrs_es::persist::PersistenceError;
use cqrs_es::{Aggregate, Query, View};
use std::collections::HashMap;
use std::sync::Arc;

#[async_trait]
pub trait CQRS<A: Aggregate, V: View<A>> {
    async fn new(queries: Vec<Box<dyn Query<A>>>, services: A::Services) -> ApplicationState<A, V>
    where
        Self: Sized;
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
pub type ApplicationState<A, V> = Arc<dyn CQRS<A, V> + Send + Sync>;
