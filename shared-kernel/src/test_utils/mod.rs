use crate::command_handler::CommandExecutor;
use async_trait::async_trait;
use cqrs_es::{Aggregate, AggregateError};
use std::collections::HashMap;

pub mod in_memory;

pub struct MockCommandHandler;

#[async_trait]
impl<A> CommandExecutor<A> for MockCommandHandler
where
    A: Aggregate + Send + Sync + 'static,
    <A as Aggregate>::Command: Send,
{
    async fn execute_with_metadata(
        &self,
        _aggregate_id: &str,
        _command: A::Command,
        _metadata: HashMap<String, String>,
    ) -> Result<(), AggregateError<A::Error>> {
        Ok(())
    }
}
