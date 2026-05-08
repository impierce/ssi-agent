use cqrs_es::{
    persist::{PersistenceError, ViewRepository},
    Aggregate, AggregateError, View,
};
use shared_kernel::authorization::{Actor, AuthorizationChecker, AuthorizationOperation, AuthorizationRequest};
use std::{collections::HashMap, sync::Arc};
use time::format_description::well_known::Rfc3339;
use tracing::{debug, error, info};

use crate::application_state::CommandHandler;

/// The `query_handler` function is used to query the view repository for a specific view.
pub async fn query_handler<A, V>(
    view_id: &str,
    state: &Arc<dyn ViewRepository<V, A>>,
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
pub enum CommandHandlerError<E>
where
    E: std::error::Error,
{
    #[error("Forbidden")]
    Forbidden,
    #[error(transparent)]
    Aggregate(#[from] AggregateError<E>),
}

/// The `command_handler` function is used to execute a command on an aggregate.
pub async fn command_handler<A>(
    authorization_checker: Arc<dyn AuthorizationChecker>,
    aggregate_id: &str,
    state: &CommandHandler<A>,
    command: <A as Aggregate>::Command,
) -> Result<(), CommandHandlerError<<A as Aggregate>::Error>>
where
    A: Aggregate,
    <A as Aggregate>::Command: Send + Sync + std::fmt::Debug,
{
    command_handler_with_authorization(authorization_checker, None, aggregate_id, state, command).await
}

pub async fn command_handler_with_authorization<A>(
    authorization_checker: Arc<dyn AuthorizationChecker>,
    actor: Option<Actor>,
    aggregate_id: &str,
    state: &CommandHandler<A>,
    command: <A as Aggregate>::Command,
) -> Result<(), CommandHandlerError<<A as Aggregate>::Error>>
where
    A: Aggregate,
    <A as Aggregate>::Command: Send + Sync + std::fmt::Debug,
{
    let authorization_request = AuthorizationRequest {
        actor,
        operation: AuthorizationOperation::Command {
            aggregate_id: aggregate_id.to_string(),
            command_type: std::any::type_name::<A::Command>(),
        },
    };

    if !authorization_checker.is_authorized(&authorization_request).await {
        return Err(CommandHandlerError::Forbidden);
    }

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
    use cqrs_es::DomainEvent;
    use serde::{Deserialize, Serialize};
    use shared_kernel::authorization::AllowAllAuthorizationChecker;
    use std::sync::Mutex;

    #[derive(Default, Debug, Serialize, Deserialize)]
    struct TestAggregate;

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

    #[async_trait]
    impl Aggregate for TestAggregate {
        type Command = String;
        type Event = TestEvent;
        type Error = TestError;
        type Services = ();

        fn aggregate_type() -> String {
            "test".to_string()
        }

        async fn handle(
            &self,
            command: Self::Command,
            _service: &Self::Services,
        ) -> Result<Vec<Self::Event>, Self::Error> {
            if command == "emit" {
                return Ok(vec![TestEvent]);
            }

            Ok(vec![])
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
        command: String,
        metadata: HashMap<String, String>,
    }

    #[async_trait]
    impl Command<TestAggregate> for CapturingCommandHandler {
        async fn execute_with_metadata(
            &self,
            aggregate_id: &str,
            command: String,
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

    struct DenyAllAuthorizationChecker;

    #[async_trait]
    impl AuthorizationChecker for DenyAllAuthorizationChecker {
        async fn is_authorized(&self, _request: &AuthorizationRequest) -> bool {
            false
        }
    }

    struct CapturingAuthorizationChecker {
        requests: Arc<Mutex<Vec<AuthorizationRequest>>>,
    }

    #[async_trait]
    impl AuthorizationChecker for CapturingAuthorizationChecker {
        async fn is_authorized(&self, request: &AuthorizationRequest) -> bool {
            self.requests.lock().unwrap().push(request.clone());
            true
        }
    }

    #[tokio::test]
    async fn command_handler_executes_authorized_command() {
        let handler = Arc::new(CapturingCommandHandler::default());
        let handler_ref: CommandHandler<TestAggregate> = handler.clone();

        command_handler(
            Arc::new(AllowAllAuthorizationChecker),
            "aggregate-id",
            &handler_ref,
            "emit".to_string(),
        )
        .await
        .unwrap();

        let calls = handler.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].aggregate_id, "aggregate-id");
        assert_eq!(calls[0].command, "emit");
        assert!(calls[0].metadata.contains_key("timestamp"));
    }

    #[tokio::test]
    async fn command_handler_with_authorization_returns_forbidden_when_denied() {
        let handler = Arc::new(CapturingCommandHandler::default());
        let command_handler: CommandHandler<TestAggregate> = handler.clone();

        let result = command_handler_with_authorization(
            Arc::new(DenyAllAuthorizationChecker),
            None,
            "aggregate-id",
            &command_handler,
            "emit".to_string(),
        )
        .await;

        assert!(matches!(result, Err(CommandHandlerError::Forbidden)));
    }

    #[tokio::test]
    async fn command_handler_with_authorization_does_not_execute_denied_command() {
        let handler = Arc::new(CapturingCommandHandler::default());
        let command_handler: CommandHandler<TestAggregate> = handler.clone();

        let _ = command_handler_with_authorization(
            Arc::new(DenyAllAuthorizationChecker),
            None,
            "aggregate-id",
            &command_handler,
            "emit".to_string(),
        )
        .await;

        assert!(handler.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn command_handler_with_authorization_sends_authorization_request() {
        let handler = Arc::new(CapturingCommandHandler::default());
        let command_handler: CommandHandler<TestAggregate> = handler.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let actor = Actor {
            subject: "user@example.test".to_string(),
        };

        command_handler_with_authorization(
            Arc::new(CapturingAuthorizationChecker {
                requests: Arc::clone(&requests),
            }),
            Some(actor.clone()),
            "aggregate-id",
            &command_handler,
            "emit".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(
            requests.lock().unwrap().as_slice(),
            &[AuthorizationRequest {
                actor: Some(actor),
                operation: AuthorizationOperation::Command {
                    aggregate_id: "aggregate-id".to_string(),
                    command_type: std::any::type_name::<String>(),
                },
            }]
        );
    }
}
