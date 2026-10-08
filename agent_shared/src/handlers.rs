use cqrs_es::{persist::PersistenceError, Aggregate, AggregateError, View};
use shared_kernel::authorization::{
    AuthorizationChecker, AuthorizationError, AuthorizationOperation, AuthorizationRequest, Caller, CommandOperation,
    QueryOperation,
};
use shared_kernel::view_repository::{load_by_id, DynViewRepository, SoftDeletable};
use std::{collections::HashMap, sync::Arc};
use time::format_description::well_known::Rfc3339;
use tracing::{debug, error, info};

use crate::application_state::CommandHandler;

/// Loads a specific view from the view repository without running authorization.
pub async fn public_query_handler<A, V>(
    view_id: &str,
    state: &Arc<dyn DynViewRepository<V, A>>,
) -> Result<Option<V>, PersistenceError>
where
    A: Aggregate,
    V: View<A>,
{
    match state.load(view_id).await {
        Ok(view) => {
            debug!("View: {:#?}\n", view);
            Ok(view)
        }
        Err(err) => {
            error!("Error: {:#?}\n", err);
            Err(err)
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum QueryHandlerError {
    #[error(transparent)]
    Authorization(AuthorizationError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

/// The `query_handler` function is used to query the view repository after authorization.
pub async fn query_handler<A, V>(
    authorization_checker: Arc<dyn AuthorizationChecker>,
    caller: Caller,
    view_id: &str,
    resource_id: Option<&str>,
    state: &Arc<dyn DynViewRepository<V, A>>,
) -> Result<Option<V>, QueryHandlerError>
where
    A: Aggregate,
    V: View<A> + QueryOperation,
{
    authorize_query::<V>(authorization_checker.as_ref(), caller, resource_id).await?;

    public_query_handler(view_id, state)
        .await
        .map_err(QueryHandlerError::Persistence)
}

/// Like [`query_handler`], but treats a soft-deleted view as missing (see [`load_by_id`]).
pub async fn live_query_handler<A, V>(
    authorization_checker: &dyn AuthorizationChecker,
    caller: Caller,
    view_id: &str,
    resource_id: Option<&str>,
    state: &dyn DynViewRepository<V, A>,
) -> Result<Option<V>, QueryHandlerError>
where
    A: Aggregate,
    V: View<A> + QueryOperation + SoftDeletable,
{
    authorize_query::<V>(authorization_checker, caller, resource_id).await?;

    load_by_id(state, view_id).await.map_err(|err| {
        error!("Error: {:#?}\n", err);
        QueryHandlerError::Persistence(err)
    })
}

async fn authorize_query<V: QueryOperation>(
    authorization_checker: &dyn AuthorizationChecker,
    caller: Caller,
    resource_id: Option<&str>,
) -> Result<(), QueryHandlerError> {
    let authorization_request = AuthorizationRequest {
        caller,
        operation: AuthorizationOperation::Query {
            resource_id: resource_id.map(str::to_owned),
            operation_name: V::OPERATION_NAME,
        },
    };

    authorization_checker
        .is_authorized(&authorization_request)
        .await
        .map_err(QueryHandlerError::Authorization)
}

#[derive(Debug, thiserror::Error)]
pub enum CommandHandlerError<E>
where
    E: std::error::Error,
{
    #[error(transparent)]
    Authorization(AuthorizationError),
    #[error(transparent)]
    Aggregate(#[from] AggregateError<E>),
}

/// Executes a command on an aggregate after verifying that the [`Caller`] is authorized.
///
/// This is the standard entry point for protected API endpoints. It validates the caller's
/// permissions against [`AuthorizationChecker`] using the command's [`CommandOperation::operation_name`],
/// and upon successful authorization, delegates to [`command_handler_with_caller`] to execute the
/// command with attached caller metadata.
///
/// # Errors
///
/// Returns a [`CommandHandlerError::Authorization`] if the caller is not permitted to perform the
/// operation, or a [`CommandHandlerError::Aggregate`] if command execution fails on the aggregate.
pub async fn command_handler<A>(
    authorization_checker: Arc<dyn AuthorizationChecker>,
    caller: Caller,
    aggregate_id: &str,
    state: &CommandHandler<A>,
    command: A::Command,
) -> Result<(), CommandHandlerError<<A as Aggregate>::Error>>
where
    A: Aggregate,
    <A as Aggregate>::Command: Send + Sync + std::fmt::Debug + CommandOperation,
{
    let operation_name = command.operation_name();
    let authorization_request = AuthorizationRequest {
        caller: caller.clone(),
        operation: AuthorizationOperation::Command {
            aggregate_id: aggregate_id.to_string(),
            resource_id: None,
            operation_name,
        },
    };

    authorization_checker
        .is_authorized(&authorization_request)
        .await
        .map_err(CommandHandlerError::Authorization)?;

    command_handler_with_caller(aggregate_id, state, command, &caller).await
}

/// Executes a command on an aggregate directly with caller provenance metadata, bypassing authorization.
///
/// This serves as the low-level execution engine for all command dispatches. It stamps the CQRS event
/// metadata with the current UTC timestamp and the [`Caller`]'s provenance (`callerid` and `callertype`),
/// ensuring that all emitted domain events and downstream integration events maintain an audit trail.
///
/// # Errors
///
/// Returns a [`CommandHandlerError::Aggregate`] if command execution or event persistence fails.
async fn command_handler_with_caller<A>(
    aggregate_id: &str,
    state: &CommandHandler<A>,
    command: A::Command,
    caller: &Caller,
) -> Result<(), CommandHandlerError<<A as Aggregate>::Error>>
where
    A: Aggregate,
    <A as Aggregate>::Command: Send + Sync + std::fmt::Debug,
{
    let mut metadata = HashMap::new();
    let timestamp = time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|err| CommandHandlerError::Aggregate(AggregateError::UnexpectedError(Box::new(err))))?;
    metadata.insert("timestamp".to_string(), timestamp);

    match caller {
        Caller::Actor(actor) => {
            metadata.insert("callerid".to_string(), actor.id().to_string());
            metadata.insert("callertype".to_string(), actor.type_name().to_string());
        }
        Caller::Internal => {
            metadata.insert("callertype".to_string(), "internal".to_string());
        }
        Caller::Anonymous => {
            metadata.insert("callertype".to_string(), "anonymous".to_string());
        }
    }

    info!("Executing command: {:?}", command);
    state
        .execute_with_metadata(aggregate_id, command, metadata)
        .await
        .map_err(CommandHandlerError::Aggregate)
        .inspect_err(|err| error!("Error: {}", err.to_string()))
}

/// Executes a command on an aggregate for public, unauthenticated protocol endpoints.
///
/// Certain decentralized protocols (e.g., OpenID4VCI credential issuance or public invitation acceptance)
/// require endpoints to be openly accessible to external wallets without authentication credentials.
///
/// This function bypasses [`AuthorizationChecker`] and delegates to [`command_handler_with_caller`]
/// using [`Caller::Anonymous`], guaranteeing that resulting events are explicitly stamped with
/// `callertype = "anonymous"`.
///
/// # Errors
///
/// Returns a [`CommandHandlerError::Aggregate`] if command execution fails on the aggregate.
pub async fn public_command_handler<A>(
    aggregate_id: &str,
    state: &CommandHandler<A>,
    command: A::Command,
) -> Result<(), CommandHandlerError<<A as Aggregate>::Error>>
where
    A: Aggregate,
    <A as Aggregate>::Command: Send + Sync + std::fmt::Debug,
{
    command_handler_with_caller(aggregate_id, state, command, &Caller::Anonymous).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application_state::Command;
    use async_trait::async_trait;
    use cqrs_es::persist::{ViewContext, ViewRepository};
    use cqrs_es::{event_sink::EventSink, DomainEvent};
    use serde::{Deserialize, Serialize};
    use shared_kernel::authorization::{Actor, AllowAllAuthorizationChecker, Caller};
    use shared_kernel::test_utils::in_memory::MemViewRepository;
    use std::sync::Mutex;

    #[derive(Default, Debug, Serialize, Deserialize)]
    struct TestAggregate;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestCommand(String);

    impl CommandOperation for TestCommand {
        fn operation_name(&self) -> &'static str {
            "test.commands.emit"
        }
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    struct TestView;

    impl View<TestAggregate> for TestView {
        fn update(&mut self, _event: &cqrs_es::EventEnvelope<TestAggregate>) {}
    }

    impl QueryOperation for TestView {
        const OPERATION_NAME: &'static str = "test.queries.get";
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestEvent;

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> String {
            "test-event".to_string()
        }

        fn event_version(&self) -> String {
            "1".to_string()
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("test error")]
    struct TestError;

    impl Aggregate for TestAggregate {
        type Command = TestCommand;
        type Event = TestEvent;
        type Error = TestError;
        type Services = ();

        const TYPE: &'static str = "test";

        async fn handle(
            &mut self,
            command: Self::Command,
            _service: &Self::Services,
            sink: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            if command.0 == "emit" {
                sink.write(TestEvent, self).await;
            }

            Ok(())
        }

        fn apply(&mut self, _event: Self::Event) {}
    }

    #[derive(Default)]
    struct CapturingCommandHandler {
        calls: Mutex<Vec<CapturedCommand>>,
    }

    #[derive(Debug, PartialEq)]
    struct CapturedCommand {
        aggregate_id: String,
        command: TestCommand,
        metadata: HashMap<String, String>,
    }

    #[async_trait]
    impl Command<TestAggregate> for CapturingCommandHandler {
        async fn execute_with_metadata(
            &self,
            aggregate_id: &str,
            command: TestCommand,
            metadata: HashMap<String, String>,
        ) -> Result<(), AggregateError<TestError>> {
            self.calls.lock().unwrap().push(CapturedCommand {
                aggregate_id: aggregate_id.to_string(),
                command,
                metadata,
            });

            Ok(())
        }
    }

    struct TestViewRepository;

    #[async_trait]
    impl DynViewRepository<TestView, TestAggregate> for TestViewRepository {
        async fn load(&self, _view_id: &str) -> Result<Option<TestView>, PersistenceError> {
            Ok(Some(TestView))
        }

        async fn load_with_context(
            &self,
            _view_id: &str,
        ) -> Result<Option<(TestView, cqrs_es::persist::ViewContext)>, PersistenceError> {
            unreachable!("query_handler only loads views")
        }

        async fn update_view(
            &self,
            _view: TestView,
            _context: cqrs_es::persist::ViewContext,
        ) -> Result<(), PersistenceError> {
            unreachable!("query_handler only loads views")
        }
    }

    struct DenyAllAuthorizationChecker;

    #[async_trait]
    impl AuthorizationChecker for DenyAllAuthorizationChecker {
        async fn is_authorized(&self, _request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            Err(AuthorizationError::Forbidden)
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

    #[tokio::test]
    async fn command_handler_executes_authorized_command() {
        let handler = Arc::new(CapturingCommandHandler::default());
        let handler_ref: CommandHandler<TestAggregate> = handler.clone();
        let authorization_checker: Arc<dyn AuthorizationChecker> = Arc::new(AllowAllAuthorizationChecker);

        command_handler(
            authorization_checker,
            Caller::Anonymous,
            "aggregate-id",
            &handler_ref,
            TestCommand("emit".to_string()),
        )
        .await
        .unwrap();

        let calls = handler.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].aggregate_id, "aggregate-id");
        assert_eq!(calls[0].command, TestCommand("emit".to_string()));
        assert!(calls[0].metadata.contains_key("timestamp"));
    }

    #[tokio::test]
    async fn command_handler_returns_forbidden_when_denied() {
        let handler = Arc::new(CapturingCommandHandler::default());
        let state: CommandHandler<TestAggregate> = handler.clone();

        let result = command_handler(
            Arc::new(DenyAllAuthorizationChecker),
            Caller::Anonymous,
            "aggregate-id",
            &state,
            TestCommand("emit".to_string()),
        )
        .await;

        assert!(matches!(
            result,
            Err(CommandHandlerError::Authorization(AuthorizationError::Forbidden))
        ));
    }

    #[tokio::test]
    async fn command_handler_does_not_execute_denied_command() {
        let handler = Arc::new(CapturingCommandHandler::default());
        let state: CommandHandler<TestAggregate> = handler.clone();

        let _ = command_handler(
            Arc::new(DenyAllAuthorizationChecker),
            Caller::Anonymous,
            "aggregate-id",
            &state,
            TestCommand("emit".to_string()),
        )
        .await;

        assert!(handler.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn command_handler_sends_authorization_request() {
        let handler = Arc::new(CapturingCommandHandler::default());
        let state: CommandHandler<TestAggregate> = handler.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let actor = Actor::user("user@example.test");

        command_handler(
            Arc::new(CapturingAuthorizationChecker {
                requests: Arc::clone(&requests),
            }),
            Caller::Actor(actor.clone()),
            "aggregate-id",
            &state,
            TestCommand("emit".to_string()),
        )
        .await
        .unwrap();

        assert_eq!(
            requests.lock().unwrap().as_slice(),
            &[AuthorizationRequest {
                caller: Caller::Actor(actor),
                operation: AuthorizationOperation::Command {
                    aggregate_id: "aggregate-id".to_string(),
                    resource_id: None,
                    operation_name: "test.commands.emit",
                },
            }]
        );
    }

    #[tokio::test]
    async fn query_handler_sends_stable_operation_name() {
        let state: Arc<dyn DynViewRepository<TestView, TestAggregate>> = Arc::new(TestViewRepository);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let actor = Actor::user("user@example.test");

        let view = query_handler(
            Arc::new(CapturingAuthorizationChecker {
                requests: Arc::clone(&requests),
            }),
            Caller::Actor(actor.clone()),
            "view-id",
            Some("resource-id"),
            &state,
        )
        .await
        .unwrap();

        assert_eq!(view, Some(TestView));
        assert_eq!(
            requests.lock().unwrap().as_slice(),
            &[AuthorizationRequest {
                caller: Caller::Actor(actor),
                operation: AuthorizationOperation::Query {
                    resource_id: Some("resource-id".to_string()),
                    operation_name: "test.queries.get",
                },
            }]
        );
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    struct SoftDeletableTestView {
        deleted: bool,
    }

    impl View<TestAggregate> for SoftDeletableTestView {
        fn update(&mut self, _event: &cqrs_es::EventEnvelope<TestAggregate>) {}
    }

    impl QueryOperation for SoftDeletableTestView {
        const OPERATION_NAME: &'static str = "test.queries.get";
    }

    impl SoftDeletable for SoftDeletableTestView {
        fn is_deleted(&self) -> bool {
            self.deleted
        }
    }

    async fn soft_deletable_views() -> MemViewRepository<SoftDeletableTestView, TestAggregate> {
        let repo = MemViewRepository::default();
        for (view_id, deleted) in [("live", false), ("deleted", true)] {
            ViewRepository::update_view(
                &repo,
                SoftDeletableTestView { deleted },
                ViewContext::new(view_id.to_string(), 0),
            )
            .await
            .unwrap();
        }
        repo
    }

    #[tokio::test]
    async fn live_query_handler_hides_soft_deleted_views() {
        let repo = soft_deletable_views().await;
        let authorization_checker = AllowAllAuthorizationChecker;

        let live = live_query_handler(&authorization_checker, Caller::Anonymous, "live", Some("live"), &repo)
            .await
            .unwrap();
        let deleted = live_query_handler(
            &authorization_checker,
            Caller::Anonymous,
            "deleted",
            Some("deleted"),
            &repo,
        )
        .await
        .unwrap();

        assert_eq!(live, Some(SoftDeletableTestView { deleted: false }));
        assert_eq!(deleted, None);
    }

    #[tokio::test]
    async fn live_query_handler_returns_forbidden_when_denied() {
        let repo = soft_deletable_views().await;

        let result = live_query_handler(
            &DenyAllAuthorizationChecker,
            Caller::Anonymous,
            "live",
            Some("live"),
            &repo,
        )
        .await;

        assert!(matches!(
            result,
            Err(QueryHandlerError::Authorization(AuthorizationError::Forbidden))
        ));
    }

    #[tokio::test]
    async fn command_handler_with_caller_attaches_user_caller_metadata_with_explicit_type() {
        let handler = Arc::new(CapturingCommandHandler::default());
        let state: CommandHandler<TestAggregate> = handler.clone();
        let caller = Caller::Actor(Actor::user("user-uuid-456"));

        command_handler_with_caller("agg-1", &state, TestCommand("emit".to_string()), &caller)
            .await
            .unwrap();

        let calls = handler.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].metadata.get("callerid"), Some(&"user-uuid-456".to_string()));
        assert_eq!(calls[0].metadata.get("callertype"), Some(&"user".to_string()));
    }

    #[tokio::test]
    async fn command_handler_with_caller_attaches_service_account_caller_metadata() {
        let handler = Arc::new(CapturingCommandHandler::default());
        let state: CommandHandler<TestAggregate> = handler.clone();
        let caller = Caller::Actor(Actor::service_account("sa-456"));

        command_handler_with_caller("agg-1", &state, TestCommand("emit".to_string()), &caller)
            .await
            .unwrap();

        let calls = handler.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].metadata.get("callerid"), Some(&"sa-456".to_string()));
        assert_eq!(
            calls[0].metadata.get("callertype"),
            Some(&"service-account".to_string())
        );
    }

    #[tokio::test]
    async fn command_handler_with_caller_attaches_internal_caller_metadata() {
        let handler = Arc::new(CapturingCommandHandler::default());
        let state: CommandHandler<TestAggregate> = handler.clone();

        command_handler_with_caller("agg-1", &state, TestCommand("emit".to_string()), &Caller::Internal)
            .await
            .unwrap();

        let calls = handler.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].metadata.get("callerid"), None);
        assert_eq!(calls[0].metadata.get("callertype"), Some(&"internal".to_string()));
    }

    #[tokio::test]
    async fn command_handler_with_caller_attaches_anonymous_caller_metadata() {
        let handler = Arc::new(CapturingCommandHandler::default());
        let state: CommandHandler<TestAggregate> = handler.clone();

        command_handler_with_caller("agg-1", &state, TestCommand("emit".to_string()), &Caller::Anonymous)
            .await
            .unwrap();

        let calls = handler.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].metadata.get("callerid"), None);
        assert_eq!(calls[0].metadata.get("callertype"), Some(&"anonymous".to_string()));
    }
}
