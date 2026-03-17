use async_trait::async_trait;
use chrono::Utc;
use cqrs_es::{Aggregate, AggregateError, CqrsFramework, EventStore, Query};
use std::{collections::HashMap, future::Future, sync::Arc};
use tracing::{error, info};

/// The `CommandExecutor` trait is used by the application service to execute commands on aggregates.
#[async_trait]
pub trait CommandExecutor<A>
where
    A: Aggregate,
{
    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: A::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), AggregateError<A::Error>>;
}

/// A type alias for a thread-safe, shared reference to a [`CommandExecutor`].
pub type CommandHandler<A> = Arc<dyn CommandExecutor<A> + Send + Sync>;

/// Dispatch a command to a [`CommandHandler`] with standard metadata (timestamp).
///
/// This eliminates the repetitive metadata construction and result mapping
/// that every `handle_command` implementation would otherwise duplicate.
pub async fn dispatch<A>(
    handler: &CommandHandler<A>,
    aggregate_id: &str,
    command: A::Command,
) -> Result<(), AggregateError<A::Error>>
where
    A: Aggregate,
    A::Command: Send,
{
    let metadata: HashMap<String, String> = [("timestamp".to_string(), Utc::now().to_rfc3339())]
        .into_iter()
        .collect();

    handler
        .execute_with_metadata(aggregate_id, command, metadata)
        .await
        .inspect(|()| info!("Command executed successfully for aggregate: {aggregate_id}"))
        .inspect_err(|e| error!("Command execution failed: {e:?}"))
}

#[async_trait]
impl<A, ES> CommandExecutor<A> for CqrsFramework<A, ES>
where
    A: Aggregate,
    ES: EventStore<A>,
    <ES as EventStore<A>>::AC: Send,
    <A as Aggregate>::Command: Send,
{
    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: A::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), AggregateError<A::Error>> {
        self.execute_with_metadata(aggregate_id, command, metadata).await
    }
}

/// A factory trait for creating [`CommandHandler`] instances backed by a specific store.
///
/// Each store backend (InMemory, etc.) implements this once.
/// Bounded context builders use it to construct aggregate handlers for all their
/// aggregates without coupling to a specific store implementation.
pub trait CommandHandlerFactory: Send + Sync {
    fn create_handler<A>(
        &self,
        services: A::Services,
        queries: Vec<Box<dyn Query<A>>>,
    ) -> impl Future<Output = CommandHandler<A>> + Send
    where
        A: Aggregate + 'static,
        <A as Aggregate>::Command: Send;
}
