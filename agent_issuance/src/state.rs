use async_trait::async_trait;
use cqrs_es::persist::PersistenceError;
use cqrs_es::{Aggregate, Query, View};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use crate::handlers::command_handler;

#[allow(clippy::new_ret_no_self)]
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

/// Initialize the application state by executing the startup commands.
pub async fn initialize<A: Aggregate, V: View<A>>(state: ApplicationState<A, V>, startup_commands: Vec<A::Command>)
where
    <A as Aggregate>::Command: Send + Sync + std::fmt::Debug,
{
    info!("Initializing ...");

    for command in startup_commands {
        let command_string = format!("{:?}", command).split(' ').next().unwrap().to_string();
        match command_handler("agg-id-F39A0C".to_string(), &state, command).await {
            Ok(_) => info!("Startup task completed: `{}`", command_string),
            Err(err) => warn!("Startup task failed: {:#?}", err),
        }
    }
}
