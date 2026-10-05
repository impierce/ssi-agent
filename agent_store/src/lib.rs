use agent_shared::application_state::Command;
pub use agent_shared::application_state::{CqrsComponentBuilder, CqrsComponents};
use agent_shared::custom_queries::ListAllQuery;
use agent_shared::generic_query::generic_query;
use async_trait::async_trait;
use cqrs_es::persist::ViewRepository;
use cqrs_es::{Aggregate, CqrsFramework, EventEnvelope, EventStore, Query, View};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

pub mod event_verification;
pub mod in_memory;
pub mod mongodb;
pub mod postgres;

pub use mongodb::MongoEventSource;

pub struct SimpleLoggingQuery;

#[async_trait]
impl<A: Aggregate> Query<A> for SimpleLoggingQuery {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<A>]) {
        for event in events {
            let payload = serde_json::to_string_pretty(&event.payload).unwrap();
            info!("{}-{} - {}", aggregate_id, event.sequence, payload);
        }
    }
}

/// A generic command handler for a specific aggregate.
///
/// This struct wraps the `CqrsFramework` to provide a unified entry point
/// for executing commands and configuring query-side processors (views).
pub struct AggregateHandler<A, CCB>
where
    A: Aggregate,
    CCB: EventStore<A> + Send + Sync + 'static,
{
    pub cqrs: CqrsFramework<A, CCB>,
}

/// Implements the `Command` trait to allow the handler to execute commands.
///
/// This implementation simply delegates the call to the underlying `CqrsFramework`.
#[async_trait]
impl<A, CCB> Command<A> for AggregateHandler<A, CCB>
where
    A: Aggregate,
    CCB: EventStore<A>,
    <CCB as EventStore<A>>::AC: Send,
    <A as Aggregate>::Command: Send,
{
    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: A::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<A::Error>> {
        self.cqrs.execute_with_metadata(aggregate_id, command, metadata).await
    }
}

impl<A, CCB> AggregateHandler<A, CCB>
where
    A: Aggregate + 'static,
    CCB: EventStore<A>,
    <A as Aggregate>::Command: Send,
{
    /// Appends a query processor (e.g., a view generator) to the CQRS framework.
    ///
    /// This is used to register components that listen to events and update read models.
    fn append_query<Q>(self, query: Q) -> Self
    where
        Q: Query<A> + 'static,
    {
        Self {
            cqrs: self.cqrs.append_query(Box::new(query)),
        }
    }

    /// Appends a dynamically dispatched query.
    fn append_boxed_query(self, query: Box<dyn Query<A>>) -> Self {
        Self {
            cqrs: self.cqrs.append_query(query),
        }
    }

    /// A convenience method to configure the handler with standard queries and custom queries.
    ///
    /// This wires up the default queries for logging, single-aggregate views, and all-aggregate views,
    /// and then folds in any additional queries provided.
    fn with_parameters<V, AV, VR1, VR2>(
        self,
        aggregate: Arc<VR1>,
        all_aggregates: Arc<VR2>,
        queries: Vec<Box<dyn Query<A>>>,
        all_aggregates_name: &str,
    ) -> Self
    where
        V: View<A> + 'static,
        AV: View<A> + 'static,
        VR1: ViewRepository<V, A> + 'static,
        VR2: ViewRepository<AV, A> + 'static,
    {
        queries.into_iter().fold(
            self.append_query(SimpleLoggingQuery)
                .append_query(generic_query(aggregate.clone()))
                .append_query(ListAllQuery::new(all_aggregates.clone(), all_aggregates_name)),
            |aggregate_handler, query| aggregate_handler.append_boxed_query(query),
        )
    }
}
