use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use crate::authorization::{
    AuthorizationCheckerFor, AuthorizationConfigurationError, AuthorizationError, AuthorizationOperation,
    AuthorizationRequest, Caller,
};

/// Defines the domain-specific types and handlers for a bounded context.
///
/// Each bounded context implements this trait to declare its command/query types
/// and how they are processed. The [`ApplicationService`] then drives the execution
/// loop, receiving envelopes from presentation-layer handles and delegating to this context.
///
/// # Associated Types
///
/// - **`Command`** / **`Query`** — the write and read messages the context accepts.
/// - **`View`** — the read-model returned by queries.
/// - **`CommandError`** / **`QueryError`** — domain-specific error types.
#[async_trait]
pub trait ApplicationContext: Send + Sync + 'static {
    // Inputs/Outputs
    type Command: Send;
    type Query: Send;
    type View: Send;

    // Errors
    type CommandError: std::error::Error + Send;
    type QueryError: std::error::Error + Send;

    /// Execute a write-side command against the aggregate identified by `aggregate_id`.
    ///
    /// Returns the (possibly newly-created) aggregate ID on success.
    async fn handle_command(&self, aggregate_id: &str, command: Self::Command) -> Result<String, Self::CommandError>;

    /// Execute a read-side query, returning the projected view.
    async fn handle_query(&self, query: Self::Query) -> Result<Self::View, Self::QueryError>;

    /// Returns the resource targeted by a command, when one can be derived.
    fn command_resource_id(&self, _command: &Self::Command) -> Option<String> {
        None
    }

    /// Returns the resource targeted by a query, when one can be derived.
    fn query_resource_id(&self, _query: &Self::Query) -> Option<String> {
        None
    }

    /// Returns the stable authorization operation name for a command.
    fn command_operation_name(&self, command: &Self::Command) -> &'static str;

    /// Returns the stable authorization operation name for a query.
    fn query_operation_name(&self, query: &Self::Query) -> &'static str;
}

/// A command message together with routing information and a reply channel.
///
/// Sent from a presentation-layer handle to the [`ApplicationService`] over an `mpsc` channel.
pub struct CommandEnvelope<AC: ApplicationContext> {
    /// The caller of the command.
    pub caller: Caller,
    /// The target aggregate ID.
    pub aggregate_id: String,
    /// The domain command to execute.
    pub command: AC::Command,
    /// One-shot channel for sending back the result.
    pub reply: oneshot::Sender<Result<String, ApplicationServiceError<AC::CommandError>>>,
}

/// A query message together with a reply channel.
///
/// Sent from a presentation-layer handle to the [`ApplicationService`] over an `mpsc` channel.
pub struct QueryEnvelope<AC: ApplicationContext> {
    /// The caller of the query.
    pub caller: Caller,
    /// The domain query to execute.
    pub query: AC::Query,
    /// One-shot channel for sending back the result.
    pub reply: oneshot::Sender<Result<AC::View, ApplicationServiceError<AC::QueryError>>>,
}

/// A shared application-service error around bounded-context command and query errors.
#[derive(Debug, thiserror::Error)]
pub enum ApplicationServiceError<E>
where
    E: std::error::Error,
{
    #[error(transparent)]
    Authorization(AuthorizationError),
    #[error(transparent)]
    Context(E),
}

/// An actor-style application service that drives an [`ApplicationContext`].
///
/// The service wraps the context in an `Arc` and processes commands and queries by
/// spawning each into its own Tokio task. This allows long-running operations
/// (e.g. outbound HTTP calls in domain services) to proceed without blocking the
/// service from accepting new messages.
///
/// # Lifecycle
///
/// 1. Create with [`ApplicationService::new`], passing the context and receiver halves.
/// 2. Spawn [`ApplicationService::start`] on the Tokio runtime.
/// 3. The service runs until **both** sender halves are dropped (channels close).
///
/// # Example
///
/// ```rust,ignore
/// use tokio::sync::mpsc;
/// use shared_kernel::application_service::{ApplicationService, CommandEnvelope, QueryEnvelope};
///
/// let (command_tx, command_rx) = mpsc::channel::<CommandEnvelope<MyContext>>(32);
/// let (query_tx, query_rx) = mpsc::channel::<QueryEnvelope<MyContext>>(32);
///
/// let service = ApplicationService::new(my_context, command_rx, query_rx, authorization_checker)?;
/// tokio::spawn(service.start());
///
/// // Send a command from the presentation layer:
/// let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
/// command_tx.send(CommandEnvelope { caller: Caller::Anonymous, aggregate_id: "aggregate-id".into(), command: my_cmd, reply: reply_tx }).await?;
/// let result = reply_rx.await?;
/// ```
pub struct ApplicationService<AC: ApplicationContext> {
    context: Arc<AC>,
    authorization_checker: Arc<dyn AuthorizationCheckerFor<AC>>,
    command_rx: mpsc::Receiver<CommandEnvelope<AC>>,
    query_rx: mpsc::Receiver<QueryEnvelope<AC>>,
}

impl<AC: ApplicationContext> ApplicationService<AC> {
    /// Creates an application service after validating its authorization configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the authorization checker is not completely configured for this
    /// application context.
    pub fn new(
        context: AC,
        command_rx: mpsc::Receiver<CommandEnvelope<AC>>,
        query_rx: mpsc::Receiver<QueryEnvelope<AC>>,
        authorization_checker: Arc<dyn AuthorizationCheckerFor<AC>>,
    ) -> Result<Self, AuthorizationConfigurationError> {
        authorization_checker.validate_context()?;

        Ok(Self {
            context: Arc::new(context),
            authorization_checker,
            command_rx,
            query_rx,
        })
    }

    /// Run the message loop, processing commands and queries until all senders are dropped.
    ///
    /// Uses `tokio::select!` to receive from both channels. Each message is spawned into
    /// its own task so that long-running commands (e.g. those making outbound HTTP calls)
    /// do not block the service from processing other messages.
    pub async fn start(mut self) {
        info!("ApplicationService started, listening for commands and queries");

        loop {
            tokio::select! {
                Some(msg) = self.command_rx.recv() => {
                    let context = Arc::clone(&self.context);
                    let authorization_checker = Arc::clone(&self.authorization_checker);
                    tokio::spawn(async move {
                        process_command(context.as_ref(), authorization_checker.as_ref(), msg).await;
                    });
                }
                Some(msg) = self.query_rx.recv() => {
                    let context = Arc::clone(&self.context);
                    let authorization_checker = Arc::clone(&self.authorization_checker);
                    tokio::spawn(async move {
                        process_query(context.as_ref(), authorization_checker.as_ref(), msg).await;
                    });
                }
                // Both channels are closed — all senders have been dropped.
                else => {
                    info!("All channels closed, shutting down ApplicationService");
                    break;
                }
            }
        }
    }
}

async fn process_command<AC: ApplicationContext>(
    context: &AC,
    authorization_checker: &dyn AuthorizationCheckerFor<AC>,
    msg: CommandEnvelope<AC>,
) {
    debug!(aggregate_id = %msg.aggregate_id, "Processing command");

    let authorization_request = AuthorizationRequest {
        caller: msg.caller.clone(),
        operation: AuthorizationOperation::Command {
            aggregate_id: msg.aggregate_id.clone(),
            resource_id: context.command_resource_id(&msg.command),
            operation_name: context.command_operation_name(&msg.command),
        },
    };
    if let Err(error) = authorization_checker
        .is_authorized(&authorization_request)
        .await
        .map_err(ApplicationServiceError::Authorization)
    {
        let _ = msg.reply.send(Err(error));
        return;
    }

    let result = context
        .handle_command(&msg.aggregate_id, msg.command)
        .await
        .map_err(ApplicationServiceError::Context);

    match &result {
        Ok(id) => info!(aggregate_id = %id, "Command executed successfully"),
        Err(e) => error!(aggregate_id = %msg.aggregate_id, error = %e, "Command execution failed"),
    }

    if msg.reply.send(result).is_err() {
        warn!(aggregate_id = %msg.aggregate_id, "Reply channel dropped before command result could be sent");
    }
}

async fn process_query<AC: ApplicationContext>(
    context: &AC,
    authorization_checker: &dyn AuthorizationCheckerFor<AC>,
    msg: QueryEnvelope<AC>,
) {
    debug!("Processing query");

    let authorization_request = AuthorizationRequest {
        caller: msg.caller.clone(),
        operation: AuthorizationOperation::Query {
            resource_id: context.query_resource_id(&msg.query),
            operation_name: context.query_operation_name(&msg.query),
        },
    };
    if let Err(error) = authorization_checker
        .is_authorized(&authorization_request)
        .await
        .map_err(ApplicationServiceError::Authorization)
    {
        let _ = msg.reply.send(Err(error));
        return;
    }

    let result = context
        .handle_query(msg.query)
        .await
        .map_err(ApplicationServiceError::Context);

    match &result {
        Ok(_) => debug!("Query executed successfully"),
        Err(e) => error!(error = %e, "Query execution failed"),
    }

    if msg.reply.send(result).is_err() {
        warn!("Reply channel dropped before query result could be sent");
    }
}

#[cfg(test)]
mod tests {
    use crate::authorization::{Actor, AllowAllAuthorizationChecker, AuthorizationChecker};
    use crate::service_registry::{ServiceError, ServiceHandle};

    use super::*;
    use std::fmt;
    use std::sync::Mutex;
    use tokio::sync::mpsc;

    // ── Fixture types ──────────────────────────────────────────────

    #[derive(Debug)]
    struct TestCommandError(String);
    impl fmt::Display for TestCommandError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for TestCommandError {}

    #[derive(Debug)]
    struct TestQueryError(String);
    impl fmt::Display for TestQueryError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for TestQueryError {}

    #[derive(Debug, PartialEq)]
    struct TestView(String);

    /// A trivial context for testing. Commands echo the aggregate ID;
    /// queries echo the query string as a view.
    struct EchoContext;

    #[async_trait]
    impl ApplicationContext for EchoContext {
        type Command = String;
        type Query = String;
        type View = TestView;
        type CommandError = TestCommandError;
        type QueryError = TestQueryError;

        async fn handle_command(
            &self,
            aggregate_id: &str,
            _command: Self::Command,
        ) -> Result<String, Self::CommandError> {
            Ok(aggregate_id.to_string())
        }

        async fn handle_query(&self, query: Self::Query) -> Result<Self::View, Self::QueryError> {
            let view = query.replace("query", "view");
            Ok(TestView(view))
        }

        fn command_resource_id(&self, command: &Self::Command) -> Option<String> {
            Some(command.clone())
        }

        fn query_resource_id(&self, query: &Self::Query) -> Option<String> {
            Some(query.clone())
        }

        fn command_operation_name(&self, _command: &Self::Command) -> &'static str {
            "test.echo.command"
        }

        fn query_operation_name(&self, _query: &Self::Query) -> &'static str {
            "test.echo.query"
        }
    }

    /// A context that always fails, to test error paths.
    struct FailingContext;

    #[async_trait]
    impl ApplicationContext for FailingContext {
        type Command = String;
        type Query = String;
        type View = TestView;
        type CommandError = TestCommandError;
        type QueryError = TestQueryError;

        async fn handle_command(
            &self,
            _aggregate_id: &str,
            _command: Self::Command,
        ) -> Result<String, Self::CommandError> {
            Err(TestCommandError("command failed".into()))
        }

        async fn handle_query(&self, _query: Self::Query) -> Result<Self::View, Self::QueryError> {
            Err(TestQueryError("query failed".into()))
        }

        fn command_operation_name(&self, _command: &Self::Command) -> &'static str {
            "test.failing.command"
        }

        fn query_operation_name(&self, _query: &Self::Query) -> &'static str {
            "test.failing.query"
        }
    }

    struct DenyAllAuthorizationChecker;

    #[async_trait]
    impl AuthorizationChecker for DenyAllAuthorizationChecker {
        async fn is_authorized(&self, _request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            Err(AuthorizationError::Forbidden)
        }
    }

    impl<Context> AuthorizationCheckerFor<Context> for DenyAllAuthorizationChecker {
        fn validate_context(&self) -> Result<(), AuthorizationConfigurationError> {
            Ok(())
        }
    }

    struct CapturingAuthorizationChecker {
        requests: Arc<Mutex<Vec<AuthorizationRequest>>>,
    }

    #[async_trait]
    impl AuthorizationChecker for CapturingAuthorizationChecker {
        async fn is_authorized(&self, request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            self.requests.lock().unwrap().push(request.clone());
            Ok(())
        }
    }

    impl<Context> AuthorizationCheckerFor<Context> for CapturingAuthorizationChecker {
        fn validate_context(&self) -> Result<(), AuthorizationConfigurationError> {
            Ok(())
        }
    }

    struct InvalidAuthorizationChecker;

    #[async_trait]
    impl AuthorizationChecker for InvalidAuthorizationChecker {
        async fn is_authorized(&self, _request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            Ok(())
        }
    }

    impl AuthorizationCheckerFor<EchoContext> for InvalidAuthorizationChecker {
        fn validate_context(&self) -> Result<(), AuthorizationConfigurationError> {
            Err(AuthorizationConfigurationError::new(
                "missing EchoContext authorization",
            ))
        }
    }

    // ── Helper ─────────────────────────────────────────────────────

    /// Spawn the service and return the sender halves for commands and queries.
    fn spawn_service<AC: ApplicationContext>(
        context: AC,
        authorization_checker: Arc<dyn AuthorizationCheckerFor<AC>>,
    ) -> ServiceHandle<AC> {
        let (command_tx, command_rx) = mpsc::channel(16);
        let (query_tx, query_rx) = mpsc::channel(16);
        let service = ApplicationService::new(context, command_rx, query_rx, authorization_checker)
            .expect("authorization configuration should be valid");
        tokio::spawn(service.start());
        ServiceHandle::new(command_tx, query_tx)
    }

    // ── Tests ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn command_returns_aggregate_id() {
        let service_handle = spawn_service(EchoContext, Arc::new(AllowAllAuthorizationChecker));

        let (reply_tx, reply_rx) = oneshot::channel();
        service_handle
            .command_tx
            .send(CommandEnvelope {
                caller: Caller::Anonymous,
                aggregate_id: "aggregate-id".into(),
                command: "create".into(),
                reply: reply_tx,
            })
            .await
            .unwrap();

        let result = reply_rx.await.unwrap();
        assert_eq!(result.ok(), Some("aggregate-id".to_string()));
    }

    #[tokio::test]
    async fn query_returns_view() {
        let service_handle = spawn_service(EchoContext, Arc::new(AllowAllAuthorizationChecker));

        let (reply_tx, reply_rx) = oneshot::channel();
        service_handle
            .query_tx
            .send(QueryEnvelope {
                caller: Caller::Anonymous,
                query: "my-query".into(),
                reply: reply_tx,
            })
            .await
            .unwrap();

        let result = reply_rx.await.unwrap();
        assert_eq!(result.ok(), Some(TestView("my-view".into())));
    }

    #[tokio::test]
    async fn command_error_is_propagated() {
        let service_handle = spawn_service(FailingContext, Arc::new(AllowAllAuthorizationChecker));

        let (reply_tx, reply_rx) = oneshot::channel();
        service_handle
            .command_tx
            .send(CommandEnvelope {
                caller: Caller::Anonymous,
                aggregate_id: "aggregate-id".into(),
                command: "bad-command".into(),
                reply: reply_tx,
            })
            .await
            .unwrap();

        let result = reply_rx.await.unwrap();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "command failed");
    }

    #[tokio::test]
    async fn query_error_is_propagated() {
        let service_handle = spawn_service(FailingContext, Arc::new(AllowAllAuthorizationChecker));

        let (reply_tx, reply_rx) = oneshot::channel();
        service_handle
            .query_tx
            .send(QueryEnvelope {
                caller: Caller::Anonymous,
                query: "bad-query".into(),
                reply: reply_tx,
            })
            .await
            .unwrap();

        let result = reply_rx.await.unwrap();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "query failed");
    }

    #[tokio::test]
    async fn denied_command_returns_forbidden_before_context_error() {
        let service_handle = spawn_service(FailingContext, Arc::new(DenyAllAuthorizationChecker));

        let result = service_handle
            .dispatch_command(Caller::Anonymous, "aggregate-id".into(), "create".into())
            .await;

        assert!(matches!(
            result,
            Err(ServiceError::Authorization(AuthorizationError::Forbidden))
        ));
    }

    #[tokio::test]
    async fn denied_query_returns_forbidden_before_context_error() {
        let service_handle = spawn_service(FailingContext, Arc::new(DenyAllAuthorizationChecker));

        let result = service_handle
            .dispatch_query(Caller::Anonymous, "my-query".into())
            .await;

        assert!(matches!(
            result,
            Err(ServiceError::Authorization(AuthorizationError::Forbidden))
        ));
    }

    #[tokio::test]
    async fn command_result_send_failure_does_not_panic() {
        let (reply_tx, reply_rx) = oneshot::channel();
        drop(reply_rx);

        process_command(
            &EchoContext,
            &AllowAllAuthorizationChecker,
            CommandEnvelope {
                caller: Caller::Anonymous,
                aggregate_id: "aggregate-id".into(),
                command: "create".into(),
                reply: reply_tx,
            },
        )
        .await;
    }

    #[tokio::test]
    async fn query_result_send_failure_does_not_panic() {
        let (reply_tx, reply_rx) = oneshot::channel();
        drop(reply_rx);

        process_query(
            &EchoContext,
            &AllowAllAuthorizationChecker,
            QueryEnvelope {
                caller: Caller::Anonymous,
                query: "my-query".into(),
                reply: reply_tx,
            },
        )
        .await;
    }

    #[tokio::test]
    async fn service_stops_when_channels_close() {
        let (command_tx, command_rx) = mpsc::channel(16);
        let (query_tx, query_rx) = mpsc::channel(16);
        drop(command_tx);
        drop(query_tx);

        ApplicationService::new(
            EchoContext,
            command_rx,
            query_rx,
            Arc::new(AllowAllAuthorizationChecker),
        )
        .expect("authorization configuration should be valid")
        .start()
        .await;
    }

    #[test]
    fn service_rejects_invalid_authorization_configuration() {
        let (_command_tx, command_rx) = mpsc::channel(16);
        let (_query_tx, query_rx) = mpsc::channel(16);

        let result = ApplicationService::new(EchoContext, command_rx, query_rx, Arc::new(InvalidAuthorizationChecker));

        assert!(matches!(
            result,
            Err(error)
                if error.to_string()
                    == "Invalid authorization configuration: missing EchoContext authorization"
        ));
    }

    #[tokio::test]
    async fn command_authorization_request_contains_caller_and_operation() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let service_handle = spawn_service(
            EchoContext,
            Arc::new(CapturingAuthorizationChecker {
                requests: Arc::clone(&requests),
            }),
        );

        let actor = Actor {
            subject: "user@example.test".to_string(),
        };
        let result = service_handle
            .dispatch_command(Caller::Actor(actor.clone()), "aggregate-id".into(), "create".into())
            .await;

        assert_eq!(result.ok(), Some("aggregate-id".to_string()));
        assert_eq!(
            requests.lock().unwrap().as_slice(),
            &[AuthorizationRequest {
                caller: Caller::Actor(actor),
                operation: AuthorizationOperation::Command {
                    aggregate_id: "aggregate-id".to_string(),
                    resource_id: Some("create".to_string()),
                    operation_name: "test.echo.command",
                },
            }]
        );
    }

    #[tokio::test]
    async fn query_authorization_request_contains_caller_and_operation() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let service_handle = spawn_service(
            EchoContext,
            Arc::new(CapturingAuthorizationChecker {
                requests: Arc::clone(&requests),
            }),
        );

        let actor = Actor {
            subject: "user@example.test".to_string(),
        };
        let result = service_handle
            .dispatch_query(Caller::Actor(actor.clone()), "my-query".into())
            .await;

        assert_eq!(result.ok(), Some(TestView("my-view".into())));
        assert_eq!(
            requests.lock().unwrap().as_slice(),
            &[AuthorizationRequest {
                caller: Caller::Actor(actor),
                operation: AuthorizationOperation::Query {
                    resource_id: Some("my-query".to_string()),
                    operation_name: "test.echo.query",
                },
            }]
        );
    }
}
