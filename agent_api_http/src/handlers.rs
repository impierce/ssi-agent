use crate::error::ErrorWrapper;
use axum::Extension;
use cqrs_es::{persist::ViewRepository, Aggregate, View};
use shared_kernel::authorization::{Actor, AllowAllAuthorizationChecker, AuthorizationChecker};
use std::sync::Arc;

pub fn request_actor(actor: &Option<Extension<Option<Actor>>>) -> Option<Actor> {
    actor.as_ref().and_then(|Extension(actor)| actor.clone())
}

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
    <A as Aggregate>::Command: Send + Sync + std::fmt::Debug,
{
    if actor.is_none() {
        return Err(ErrorWrapper::Unauthorized);
    }

    agent_shared::handlers::command_handler(authorization_checker, actor, aggregate_id, state, command)
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
    <A as Aggregate>::Command: Send + Sync + std::fmt::Debug,
{
    agent_shared::handlers::command_handler(
        Arc::new(AllowAllAuthorizationChecker),
        None,
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
    state: &Arc<dyn ViewRepository<V, A>>,
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
    state: &Arc<dyn ViewRepository<V, A>>,
) -> Result<Option<V>, ErrorWrapper<A::Error>>
where
    A: Aggregate,
    V: View<A>,
{
    if actor.is_none() {
        return Err(ErrorWrapper::Unauthorized);
    }

    agent_shared::handlers::query_handler(authorization_checker, actor, view_id, state)
        .await
        .map_err(ErrorWrapper::QueryHandlerError)
}
