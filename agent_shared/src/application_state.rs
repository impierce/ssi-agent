use std::{collections::HashMap, sync::Arc};

use axum::async_trait;
use cqrs_es::Aggregate;

#[derive(Clone)]
pub struct ApplicationState<I, V> {
    pub issuance: I,
    pub verification: V,
}

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
