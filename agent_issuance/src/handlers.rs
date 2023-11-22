use agent_store::state::ApplicationState;
use cqrs_es::{
    persist::{PersistenceError, ViewRepository},
    Aggregate, AggregateError, View,
};

pub async fn query_handler<A: Aggregate, V: View<A>>(
    credential_id: String,
    state: &ApplicationState<A, V>,
) -> Result<Option<V>, PersistenceError> {
    match state.issuance_data_query.load(&credential_id).await {
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
    state: &ApplicationState<A, V>,
    command: A::Command,
) -> Result<(), AggregateError<<A as Aggregate>::Error>> {
    state
        .cqrs
        .execute_with_metadata(&aggregate_id, command, Default::default())
        .await
}
