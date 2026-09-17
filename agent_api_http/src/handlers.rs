use crate::error::ErrorWrapper;
use cqrs_es::{Aggregate, View};
use shared_kernel::authorization::{
    Actor, AllowAllAuthorizationChecker, AuthorizationChecker, Caller, CommandOperation, QueryOperation,
};
use shared_kernel::view_repository::DynViewRepository;
use std::sync::Arc;

static ALLOW_ALL_AUTHORIZATION_CHECKER: std::sync::OnceLock<Arc<dyn AuthorizationChecker>> = std::sync::OnceLock::new();

/// Wrapping the `command_handler` function from the `agent_shared` crate to handle errors.
pub async fn command_handler<A>(
    authorization_checker: Arc<dyn AuthorizationChecker>,
    actor: Option<Actor>,
    aggregate_id: &str,
    state: &agent_shared::application_state::CommandHandler<A>,
    command: <A as cqrs_es::Aggregate>::Command,
) -> Result<(), ErrorWrapper<A::Error>>
where
    A: Aggregate,
    <A as Aggregate>::Command: Send + Sync + std::fmt::Debug + CommandOperation,
{
    let caller = actor.map_or(Caller::Anonymous, Caller::Actor);

    agent_shared::handlers::command_handler(authorization_checker, caller, aggregate_id, state, command)
        .await
        .map_err(ErrorWrapper::CommandHandlerError)
}

/// Executes a command that is a fixed, trusted continuation of an already-authorized operation.
///
/// The installed authorization checker must explicitly permit the operation for [`Caller::Internal`].
pub async fn internal_command_handler<A>(
    authorization_checker: Arc<dyn AuthorizationChecker>,
    aggregate_id: &str,
    state: &agent_shared::application_state::CommandHandler<A>,
    command: <A as cqrs_es::Aggregate>::Command,
) -> Result<(), ErrorWrapper<A::Error>>
where
    A: Aggregate,
    <A as Aggregate>::Command: Send + Sync + std::fmt::Debug + CommandOperation,
{
    agent_shared::handlers::command_handler(authorization_checker, Caller::Internal, aggregate_id, state, command)
        .await
        .map_err(ErrorWrapper::CommandHandlerError)
}

/// Executes a command for public protocol endpoints that are authorized by protocol-specific checks.
pub async fn public_command_handler<A>(
    aggregate_id: &str,
    state: &agent_shared::application_state::CommandHandler<A>,
    command: <A as cqrs_es::Aggregate>::Command,
) -> Result<(), ErrorWrapper<A::Error>>
where
    A: Aggregate,
    <A as Aggregate>::Command: Send + Sync + std::fmt::Debug + CommandOperation,
{
    agent_shared::handlers::command_handler(
        ALLOW_ALL_AUTHORIZATION_CHECKER
            .get_or_init(|| Arc::new(AllowAllAuthorizationChecker))
            .clone(),
        Caller::Anonymous,
        aggregate_id,
        state,
        command,
    )
    .await
    .map_err(ErrorWrapper::CommandHandlerError)
}

/// Executes a query for public protocol endpoints that are authorized by protocol-specific checks.
pub async fn public_query_handler<A, V>(
    view_id: &str,
    state: &Arc<dyn DynViewRepository<V, A>>,
) -> Result<Option<V>, ErrorWrapper<A::Error>>
where
    A: Aggregate,
    V: View<A>,
{
    agent_shared::handlers::public_query_handler(view_id, state)
        .await
        .map_err(ErrorWrapper::PersistenceError)
}

// Wrapping the `query_handler` function from the `agent_shared` crate to handle errors.
pub async fn query_handler<A, V>(
    authorization_checker: Arc<dyn AuthorizationChecker>,
    actor: Option<Actor>,
    view_id: &str,
    resource_id: Option<&str>,
    state: &Arc<dyn DynViewRepository<V, A>>,
) -> Result<Option<V>, ErrorWrapper<A::Error>>
where
    A: Aggregate,
    V: View<A> + QueryOperation,
{
    let caller = actor.map_or(Caller::Anonymous, Caller::Actor);

    agent_shared::handlers::query_handler(authorization_checker, caller, view_id, resource_id, state)
        .await
        .map_err(ErrorWrapper::QueryHandlerError)
}

/// Executes a query that is a fixed, trusted continuation of an already-authorized operation.
///
/// The installed authorization checker must explicitly permit the operation for [`Caller::Internal`].
pub async fn internal_query_handler<A, V>(
    authorization_checker: Arc<dyn AuthorizationChecker>,
    view_id: &str,
    resource_id: Option<&str>,
    state: &Arc<dyn DynViewRepository<V, A>>,
) -> Result<Option<V>, ErrorWrapper<A::Error>>
where
    A: Aggregate,
    V: View<A> + QueryOperation,
{
    agent_shared::handlers::query_handler(authorization_checker, Caller::Internal, view_id, resource_id, state)
        .await
        .map_err(ErrorWrapper::QueryHandlerError)
}
