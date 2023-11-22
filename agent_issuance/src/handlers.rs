use crate::state::DynApplicationState;
use cqrs_es::{persist::PersistenceError, Aggregate, AggregateError, View};

pub async fn query_handler<A: Aggregate, V: View<A>>(
    credential_id: String,
    state: &DynApplicationState<A, V>,
) -> Result<Option<V>, PersistenceError> {
    match state.load(&credential_id).await {
        Ok(view) => {
            println!("View: {:#?}\n", view);
            Ok(view)
        }
        Err(err) => {
            println!("Error: {:#?}\n", err);
            Err(err)
        }
    }
}

pub async fn command_handler<A: Aggregate, V: View<A>>(
    aggregate_id: String,
    state: &DynApplicationState<A, V>,
    command: A::Command,
) -> Result<(), AggregateError<<A as Aggregate>::Error>>
where
    A::Command: Send + Sync,
{
    state
        .execute_with_metadata(&aggregate_id, command, Default::default())
        .await
}
