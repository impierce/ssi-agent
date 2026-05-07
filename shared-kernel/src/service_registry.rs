use std::any::{Any, TypeId};
use std::collections::HashMap;
use tokio::sync::oneshot::error::RecvError;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;

use crate::application_service::{ApplicationContext, CommandEnvelope, QueryEnvelope};
use crate::authorization::Actor;

/// A type-keyed registry for storing and retrieving services at runtime.
///
/// Services are stored as `Box<dyn Any>` and keyed by their [`TypeId`], allowing
/// heterogeneous service types to coexist in a single container. This is typically
/// populated during application startup and queried by the presentation layer to
/// obtain [`ServiceHandle`]s for each bounded context.
#[derive(Default)]
pub struct ServiceRegistry(HashMap<TypeId, Box<dyn Any + Send + Sync>>);

impl ServiceRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a service of type `T`, replacing any previously registered service of the same type.
    pub fn register<T: 'static + Send + Sync>(&mut self, service: T) -> &mut Self {
        self.0.insert(TypeId::of::<T>(), Box::new(service));
        self
    }

    /// Retrieve a clone of the service registered for type `T`, or `None` if no such service exists.
    #[must_use]
    pub fn get_handle<T: 'static + Clone>(&self) -> Option<T> {
        self.0
            .get(&TypeId::of::<T>())
            .and_then(|s| s.downcast_ref::<T>())
            .cloned()
    }
}

/// Error dispatching a command or query to the application service.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError<AC: ApplicationContext> {
    #[error("Send command error: {0}")]
    SendCommandError(#[from] mpsc::error::SendError<CommandEnvelope<AC>>),
    #[error("Send query error: {0}")]
    SendQueryError(#[from] mpsc::error::SendError<QueryEnvelope<AC>>),
    #[error("Receive error: {0}")]
    RecvError(#[from] RecvError),
    #[error("Command error: {0}")]
    CommandError(AC::CommandError),
    #[error("Query error: {0}")]
    QueryError(AC::QueryError),
}

/// A cloneable handle for sending commands and queries to an [`ApplicationService`](crate::application_service::ApplicationService).
///
/// Created during startup and stored in the [`ServiceRegistry`]. The presentation
/// layer (e.g. HTTP handlers) clones this handle and uses [`dispatch_command`](ServiceHandle::dispatch_command)
/// / [`dispatch_query`](ServiceHandle::dispatch_query) to communicate with the
/// bounded context's application service over `mpsc` channels.
pub struct ServiceHandle<AC>
where
    AC: ApplicationContext,
{
    pub command_tx: mpsc::Sender<CommandEnvelope<AC>>,
    pub query_tx: mpsc::Sender<QueryEnvelope<AC>>,
}

impl<AC> ServiceHandle<AC>
where
    AC: ApplicationContext,
{
    #[must_use]
    pub fn new(command_tx: mpsc::Sender<CommandEnvelope<AC>>, query_tx: mpsc::Sender<QueryEnvelope<AC>>) -> Self {
        Self { command_tx, query_tx }
    }

    /// Send a command to the application service and await its result.
    /// # Errors
    ///
    /// Returns `ServiceError::SendCommandError` if sending the command fails.
    /// Returns `ServiceError::RecvError` if awaiting the response fails.
    /// Returns `ServiceError::CommandError` if the application service returns an error for the
    /// command.
    pub async fn dispatch_command(
        &self,
        aggregate_id: String,
        command: AC::Command,
    ) -> Result<String, ServiceError<AC>> {
        self.dispatch_command_as(None, aggregate_id, command).await
    }

    /// Send a command to the application service as an actor and await its result.
    /// # Errors
    ///
    /// Returns `ServiceError::SendCommandError` if sending the command fails.
    /// Returns `ServiceError::RecvError` if awaiting the response fails.
    /// Returns `ServiceError::CommandError` if the application service returns an error for the
    /// command.
    pub async fn dispatch_command_as(
        &self,
        actor: Option<Actor>,
        aggregate_id: String,
        command: AC::Command,
    ) -> Result<String, ServiceError<AC>> {
        debug!(%aggregate_id, "Dispatching command through service handle");

        let (reply_tx, reply_rx) = oneshot::channel();
        let command = CommandEnvelope {
            actor,
            aggregate_id,
            command,
            reply: reply_tx,
        };

        self.command_tx.send(command).await?;

        reply_rx.await?.map_err(ServiceError::CommandError)
    }

    /// Send a query to the application service and await the resulting view.
    /// # Errors
    ///
    /// Returns `ServiceError::SendQueryError` if sending the query fails.
    /// Returns `ServiceError::RecvError` if awaiting the response fails.
    /// Returns `ServiceError::QueryError` if the application service returns an error for the
    /// query.
    pub async fn dispatch_query(&self, query: AC::Query) -> Result<AC::View, ServiceError<AC>> {
        self.dispatch_query_as(None, query).await
    }

    /// Send a query to the application service as an actor and await the resulting view.
    /// # Errors
    ///
    /// Returns `ServiceError::SendQueryError` if sending the query fails.
    /// Returns `ServiceError::RecvError` if awaiting the response fails.
    /// Returns `ServiceError::QueryError` if the application service returns an error for the
    /// query.
    pub async fn dispatch_query_as(
        &self,
        actor: Option<Actor>,
        query: AC::Query,
    ) -> Result<AC::View, ServiceError<AC>> {
        debug!("Dispatching query through service handle");

        let (reply_tx, reply_rx) = oneshot::channel();
        let query = QueryEnvelope {
            actor,
            query,
            reply: reply_tx,
        };

        self.query_tx.send(query).await?;

        reply_rx.await?.map_err(ServiceError::QueryError)
    }
}

impl<AC> Clone for ServiceHandle<AC>
where
    AC: ApplicationContext,
{
    fn clone(&self) -> Self {
        Self {
            command_tx: self.command_tx.clone(),
            query_tx: self.query_tx.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application_service::{ApplicationContext, ApplicationService};
    use crate::authorization::Actor;
    use async_trait::async_trait;
    use std::fmt;
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

    #[derive(Debug, PartialEq, Clone)]
    struct TestView(String);

    #[derive(Debug)]
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
            Ok(TestView(query))
        }
    }

    // ── Tests ──────────────────────────────────────────────────────

    #[test]
    fn register_and_retrieve_service() {
        let mut registry = ServiceRegistry::new();
        registry.register(42_i32);

        assert_eq!(registry.get_handle::<i32>(), Some(42));
    }

    #[test]
    fn get_nonexistent_service_returns_none() {
        let registry = ServiceRegistry::new();

        assert_eq!(registry.get_handle::<i32>(), None);
    }

    #[test]
    fn register_overwrites_existing_service() {
        let mut registry = ServiceRegistry::new();
        registry.register(1_i32);
        registry.register(2_i32);

        assert_eq!(registry.get_handle::<i32>(), Some(2));
    }

    #[tokio::test]
    async fn dispatch_command_returns_result() {
        let (command_tx, command_rx) = mpsc::channel(16);
        let (query_tx, query_rx) = mpsc::channel(16);
        let service = ApplicationService::new(EchoContext, command_rx, query_rx);
        tokio::spawn(service.start());

        let handle = ServiceHandle::<EchoContext>::new(command_tx, query_tx);
        let result = handle.dispatch_command("aggregte-id".into(), "create".into()).await;

        assert_eq!(result.unwrap(), "aggregte-id");
    }

    #[tokio::test]
    async fn dispatch_query_returns_view() {
        let (command_tx, command_rx) = mpsc::channel(16);
        let (query_tx, query_rx) = mpsc::channel(16);
        let service = ApplicationService::new(EchoContext, command_rx, query_rx);
        tokio::spawn(service.start());

        let handle = ServiceHandle::<EchoContext>::new(command_tx, query_tx);
        let result = handle.dispatch_query("my-query".into()).await;

        assert_eq!(result.unwrap(), TestView("my-query".into()));
    }

    #[tokio::test]
    async fn dispatch_command_as_sends_actor_context() {
        let (command_tx, mut command_rx) = mpsc::channel(16);
        let (query_tx, _query_rx) = mpsc::channel(16);
        let handle = ServiceHandle::<EchoContext>::new(command_tx, query_tx);
        let actor = Actor {
            subject: "user@example.test".to_string(),
        };

        let dispatch = tokio::spawn(async move {
            handle
                .dispatch_command_as(Some(actor), "aggregate-id".into(), "create".into())
                .await
        });

        let command = command_rx.recv().await.unwrap();
        assert_eq!(
            command.actor,
            Some(Actor {
                subject: "user@example.test".to_string()
            })
        );
        assert_eq!(command.aggregate_id, "aggregate-id");
        assert_eq!(command.command, "create");

        command.reply.send(Ok("aggregate-id".into())).unwrap();

        assert_eq!(dispatch.await.unwrap().unwrap(), "aggregate-id");
    }

    #[tokio::test]
    async fn dispatch_query_as_sends_actor_context() {
        let (command_tx, _command_rx) = mpsc::channel(16);
        let (query_tx, mut query_rx) = mpsc::channel(16);
        let handle = ServiceHandle::<EchoContext>::new(command_tx, query_tx);
        let actor = Actor {
            subject: "user@example.test".to_string(),
        };

        let dispatch = tokio::spawn(async move { handle.dispatch_query_as(Some(actor), "my-query".into()).await });

        let query = query_rx.recv().await.unwrap();
        assert_eq!(
            query.actor,
            Some(Actor {
                subject: "user@example.test".to_string()
            })
        );
        assert_eq!(query.query, "my-query");

        query.reply.send(Ok(TestView("my-query".into()))).unwrap();

        assert_eq!(dispatch.await.unwrap().unwrap(), TestView("my-query".into()));
    }

    #[tokio::test]
    async fn service_handle_can_be_registered_and_retrieved() {
        let (command_tx, command_rx) = mpsc::channel(16);
        let (query_tx, query_rx) = mpsc::channel(16);
        let service = ApplicationService::new(EchoContext, command_rx, query_rx);
        tokio::spawn(service.start());

        let handle = ServiceHandle::<EchoContext>::new(command_tx, query_tx);

        let mut registry = ServiceRegistry::new();
        registry.register(handle);

        let retrieved = registry.get_handle::<ServiceHandle<EchoContext>>();
        assert!(retrieved.is_some());

        let result = retrieved
            .unwrap()
            .dispatch_command("aggregte-id".into(), "command".into())
            .await;
        assert_eq!(result.unwrap(), "aggregte-id");
    }
}
