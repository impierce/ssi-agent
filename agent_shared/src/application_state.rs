use std::{collections::HashMap, future::Future, sync::Arc};

use async_trait::async_trait;
use cqrs_es::{Aggregate, Query, View};
use shared_kernel::view_repository::DynViewRepository;

/// The `Command` trait is used to define the command handlers for the aggregates.
#[async_trait]
pub trait Command<A>
where
    A: Aggregate,
{
    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: A::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<A::Error>>
    where
        A::Command: Send + Sync;
}

pub type CommandHandler<A> = Arc<dyn Command<A> + Send + Sync>;

/// A type alias for the tuple of CQRS components for a given aggregate.
///
/// This includes the command handler, the single-instance view repository,
/// and the all-instances view repository.
pub type CqrsComponents<A, V, AV> = (
    Arc<dyn Command<A> + Send + Sync>,
    Arc<dyn DynViewRepository<V, A>>,
    Arc<dyn DynViewRepository<AV, A>>,
);

/// A trait for building the command and query infrastructure for a given aggregate.
///
/// Implementors of this trait (e.g., `InMemory`, `Postgres`, `MongoDB`) are responsible
/// for creating the full set of components needed to interact with an aggregate,
/// including the command handler and view repositories.
pub trait CqrsComponentBuilder {
    fn commands_and_queries<V: View<A> + 'static, A: Aggregate + 'static, AV: View<A> + 'static>(
        &self,
        services: A::Services,
        queries: Vec<Box<dyn Query<A>>>,
    ) -> impl Future<Output = CqrsComponents<A, V, AV>> + Send
    where
        <A as Aggregate>::Command: Send + Sync;
}
