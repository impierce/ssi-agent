use cqrs_es::{persist::PersistenceError, Aggregate, AggregateError, View};
use shared_kernel::authorization::{
    AuthorizationChecker, AuthorizationError, AuthorizationOperation, AuthorizationRequest, Caller, CommandOperation,
    QueryOperation,
};
use shared_kernel::view_repository::DynViewRepository;
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
        .map_err(QueryHandlerError::Authorization)?;

    public_query_handler(view_id, state)
        .await
        .map_err(QueryHandlerError::Persistence)
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

/// The `command_handler` function is used to execute a command on an aggregate.
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
        caller,
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

    public_command_handler(aggregate_id, state, command).await
}

pub async fn public_command_handler<A>(
    aggregate_id: &str,
    state: &CommandHandler<A>,
    command: A::Command,
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

    info!("Executing command: {:?}", command);
    state
        .execute_with_metadata(aggregate_id, command, metadata)
        .await
        .map_err(CommandHandlerError::Aggregate)
        .inspect_err(|err| error!("Error: {}", err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application_state::Command;
    use async_trait::async_trait;
    use cqrs_es::{event_sink::EventSink, DomainEvent};
    use serde::{Deserialize, Serialize};
    use shared_kernel::authorization::{Actor, AllowAllAuthorizationChecker, Caller};
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
        let actor = Actor {
            subject: "user@example.test".to_string(),
        };

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
        let actor = Actor {
            subject: "user@example.test".to_string(),
        };

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
}
