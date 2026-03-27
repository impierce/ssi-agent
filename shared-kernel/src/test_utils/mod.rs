//! Test utilities for the shared kernel.
//!
//! Enable with `features = ["test-utils"]` in your `Cargo.toml`.

use crate::command_handler::CommandExecutor;
use async_trait::async_trait;
use cqrs_es::{Aggregate, AggregateError};
use std::collections::HashMap;

pub mod in_memory;

/// A no-op [`CommandExecutor`] for unit tests.
///
/// Always returns `Ok(())` regardless of the command or aggregate ID.
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
