use async_trait::async_trait;
use chrono::Utc;
use cqrs_es::{Aggregate, AggregateError, CqrsFramework, EventStore, Query};
use std::{collections::HashMap, future::Future, sync::Arc};
use tracing::{debug, error, info};

/// Trait for executing commands on a specific aggregate type.
///
/// Implementors receive the aggregate ID, the command, and arbitrary metadata
/// (e.g. a timestamp). The shared-kernel provides a blanket implementation for
/// [`CqrsFramework`], so store backends don't need to implement this manually.
#[async_trait]
pub trait CommandExecutor<A>
where
    A: Aggregate,
{
    /// Execute a command against the aggregate identified by `aggregate_id`.
    ///
    /// `metadata` carries cross-cutting context such as timestamps or correlation IDs.
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
/// # Errors
///
/// Returns `AggregateError` if the handler fails to execute the command.
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

    debug!(aggregate_id, "Dispatching command with metadata");

    handler
        .execute_with_metadata(aggregate_id, command, metadata)
        .await
        .inspect(|()| info!(aggregate_id, "Command executed successfully"))
        .inspect_err(|e| error!(aggregate_id, error = ?e, "Command execution failed"))
}

/// Blanket implementation of [`CommandExecutor`] for [`CqrsFramework`].
///
/// Delegates directly to the framework's own `execute_with_metadata`,
/// allowing any store-backed `CqrsFramework` to serve as a [`CommandHandler`].
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
/// Each store backend (`InMemory`, `MongoDB`, etc.) implements this once.
/// Bounded context builders use it to construct aggregate handlers for all their
/// aggregates without coupling to a specific store implementation.
pub trait CommandHandlerFactory: Send + Sync {
    /// Create a new [`CommandHandler`] for aggregate `A`, wiring in the given
    /// domain services and event-driven queries.
    fn create_handler<A>(
        &self,
        services: A::Services,
        queries: Vec<Box<dyn Query<A>>>,
    ) -> impl Future<Output = CommandHandler<A>> + Send
    where
        A: Aggregate + 'static,
        <A as Aggregate>::Command: Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::{event_sink::EventSink, DomainEvent};
    use serde::{Deserialize, Serialize};
    use std::sync::Mutex;

    // ── Test aggregate ─────────────────────────────────────────

    #[derive(Default, Debug, Serialize, Deserialize)]
    struct TestAggregate;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestEvent;

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> String {
            "TestEvent".into()
        }
        fn event_version(&self) -> String {
            "1.0".into()
        }
    }

    #[derive(Debug)]
    struct TestError(String);

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for TestError {}

    impl Aggregate for TestAggregate {
        const TYPE: &'static str = "TestAggregate";
        type Command = String;
        type Event = TestEvent;
        type Error = TestError;
        type Services = ();

        async fn handle(
            &mut self,
            _command: Self::Command,
            _services: &Self::Services,
            _sink: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn apply(&mut self, _: Self::Event) {}
    }

    // ── Mock handlers ──────────────────────────────────────────

    /// Captures the metadata passed to `execute_with_metadata`.
    struct CapturingHandler {
        captured_metadata: Mutex<Option<HashMap<String, String>>>,
    }

    impl CapturingHandler {
        fn new() -> Self {
            Self {
                captured_metadata: Mutex::new(None),
            }
        }

        fn captured_metadata(&self) -> HashMap<String, String> {
            self.captured_metadata.lock().unwrap().clone().unwrap()
        }
    }

    #[async_trait]
    impl CommandExecutor<TestAggregate> for CapturingHandler {
        async fn execute_with_metadata(
            &self,
            _aggregate_id: &str,
            _command: String,
            metadata: HashMap<String, String>,
        ) -> Result<(), AggregateError<TestError>> {
            *self.captured_metadata.lock().unwrap() = Some(metadata);
            Ok(())
        }
    }

    /// Always returns an error.
    struct FailingHandler;

    #[async_trait]
    impl CommandExecutor<TestAggregate> for FailingHandler {
        async fn execute_with_metadata(
            &self,
            _aggregate_id: &str,
            _command: String,
            _metadata: HashMap<String, String>,
        ) -> Result<(), AggregateError<TestError>> {
            Err(AggregateError::UserError(TestError("deliberate failure".into())))
        }
    }

    // ── Tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn dispatch_attaches_timestamp_metadata() {
        let capturing = Arc::new(CapturingHandler::new());
        let handler: CommandHandler<TestAggregate> = capturing.clone();

        dispatch::<TestAggregate>(&handler, "agg-1", "create".into())
            .await
            .unwrap();

        let metadata = capturing.captured_metadata();
        assert!(
            metadata.contains_key("timestamp"),
            "metadata should contain a 'timestamp' key"
        );
    }

    #[tokio::test]
    async fn dispatch_returns_ok_on_success() {
        let handler = Arc::new(CapturingHandler::new()) as CommandHandler<TestAggregate>;

        let result = dispatch::<TestAggregate>(&handler, "agg-1", "create".into()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dispatch_propagates_handler_error() {
        let handler = Arc::new(FailingHandler) as CommandHandler<TestAggregate>;

        let result = dispatch::<TestAggregate>(&handler, "agg-1", "bad".into()).await;
        assert!(result.is_err());
    }
}
