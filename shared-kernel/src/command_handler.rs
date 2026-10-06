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
    dispatch_with_caller(handler, aggregate_id, command, &crate::authorization::Caller::Anonymous).await
}

/// Dispatch a command to a [`CommandHandler`] with caller provenance and standard metadata.
///
/// If `caller` is an authenticated actor, caller metadata (`callerid`, `callertype`)
/// is attached alongside the timestamp so that events emitted by the aggregate retain
/// information about who produced them.
///
/// # Errors
///
/// Returns `AggregateError` if the handler fails to execute the command.
pub async fn dispatch_with_caller<A>(
    handler: &CommandHandler<A>,
    aggregate_id: &str,
    command: A::Command,
    caller: &crate::authorization::Caller,
) -> Result<(), AggregateError<A::Error>>
where
    A: Aggregate,
    A::Command: Send,
{
    let mut metadata: HashMap<String, String> = [("timestamp".to_string(), Utc::now().to_rfc3339())]
        .into_iter()
        .collect();

    match caller {
        crate::authorization::Caller::Actor(actor) => {
            metadata.insert("callerid".to_string(), actor.id().to_string());
            metadata.insert("callertype".to_string(), actor.type_name().to_string());
        }
        crate::authorization::Caller::Internal => {
            metadata.insert("callertype".to_string(), "internal".to_string());
        }
        crate::authorization::Caller::Anonymous => {
            metadata.insert("callertype".to_string(), "anonymous".to_string());
        }
    }

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
    type Error: std::error::Error + Send + Sync + 'static;

    /// Create a new [`CommandHandler`] for aggregate `A`, wiring in the given
    /// domain services and event-driven queries.
    fn create_handler<A>(
        &self,
        services: A::Services,
        queries: Vec<Box<dyn Query<A>>>,
    ) -> impl Future<Output = Result<CommandHandler<A>, Self::Error>> + Send
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

    #[tokio::test]
    async fn dispatch_with_caller_attaches_user_caller_metadata() {
        use crate::authorization::{Actor, Caller};

        let capturing = Arc::new(CapturingHandler::new());
        let handler: CommandHandler<TestAggregate> = capturing.clone();

        let caller = Caller::Actor(Actor::user("user-uuid-123"));

        dispatch_with_caller::<TestAggregate>(&handler, "agg-1", "create".into(), &caller)
            .await
            .unwrap();

        let metadata = capturing.captured_metadata();
        assert_eq!(metadata.get("callerid"), Some(&"user-uuid-123".to_string()));
        assert_eq!(metadata.get("callertype"), Some(&"user".to_string()));
    }

    #[tokio::test]
    async fn dispatch_with_caller_attaches_service_account_caller_metadata() {
        use crate::authorization::{Actor, Caller};

        let capturing = Arc::new(CapturingHandler::new());
        let handler: CommandHandler<TestAggregate> = capturing.clone();

        let caller = Caller::Actor(Actor::service_account("sa-123"));

        dispatch_with_caller::<TestAggregate>(&handler, "agg-1", "create".into(), &caller)
            .await
            .unwrap();

        let metadata = capturing.captured_metadata();
        assert_eq!(metadata.get("callerid"), Some(&"sa-123".to_string()));
        assert_eq!(metadata.get("callertype"), Some(&"service-account".to_string()));
    }

    #[tokio::test]
    async fn dispatch_with_caller_attaches_internal_caller_metadata() {
        use crate::authorization::Caller;

        let capturing = Arc::new(CapturingHandler::new());
        let handler: CommandHandler<TestAggregate> = capturing.clone();

        dispatch_with_caller::<TestAggregate>(&handler, "agg-1", "create".into(), &Caller::Internal)
            .await
            .unwrap();

        let metadata = capturing.captured_metadata();
        assert_eq!(metadata.get("callerid"), None);
        assert_eq!(metadata.get("callertype"), Some(&"internal".to_string()));
    }

    #[tokio::test]
    async fn dispatch_with_caller_attaches_anonymous_caller_metadata() {
        use crate::authorization::Caller;

        let capturing = Arc::new(CapturingHandler::new());
        let handler: CommandHandler<TestAggregate> = capturing.clone();

        dispatch_with_caller::<TestAggregate>(&handler, "agg-1", "create".into(), &Caller::Anonymous)
            .await
            .unwrap();

        let metadata = capturing.captured_metadata();
        assert_eq!(metadata.get("callerid"), None);
        assert_eq!(metadata.get("callertype"), Some(&"anonymous".to_string()));
    }
}
