pub mod aggregate;

use agent_store::state::ApplicationState;
use cqrs_es::{Aggregate, AggregateError, View};

use crate::handlers::command_handler;

pub async fn create_credential<A: Aggregate, V: View<A>>(
    state: ApplicationState<A, V>,
    command: A::Command,
) -> Result<(), AggregateError<<A as Aggregate>::Error>> {
    command_handler("agg-id-F39A0C".to_string(), state, command).await
}
