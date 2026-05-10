use crate::error::ErrorWrapper;
use agent_shared::handlers::AuthorizationContext;
use cqrs_es::{persist::ViewRepository, Aggregate, View};
use std::sync::Arc;

/// Wrapping the `command_handler` function from the `agent_shared` crate to handle errors.
pub async fn command_handler<C, A>(
    authorization_context: &C,
    aggregate_id: &str,
    state: &agent_shared::application_state::CommandHandler<A>,
    command: <A as cqrs_es::Aggregate>::Command,
) -> Result<(), ErrorWrapper<A::Error>>
where
    C: AuthorizationContext + ?Sized,
    A: Aggregate,
    <A as Aggregate>::Command: Send + Sync + std::fmt::Debug,
{
    agent_shared::handlers::command_handler(authorization_context, aggregate_id, state, command)
        .await
        .map_err(ErrorWrapper::CommandHandlerError)
}

// Wrapping the `query_handler` function from the `agent_shared` crate to handle errors.
pub async fn query_handler<A, V>(
    view_id: &str,
    state: &Arc<dyn ViewRepository<V, A>>,
) -> Result<Option<V>, ErrorWrapper<A::Error>>
where
    A: Aggregate,
    V: View<A>,
{
    agent_shared::handlers::query_handler(view_id, state)
        .await
        .map_err(ErrorWrapper::PersistenceError)
}
