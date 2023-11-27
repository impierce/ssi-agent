use std::collections::HashMap;

use crate::state::ApplicationState;
use cqrs_es::{persist::PersistenceError, Aggregate, AggregateError, View};
use time::format_description::well_known::Rfc3339;

pub async fn query_handler<A: Aggregate, V: View<A>>(
    credential_id: String,
    state: &ApplicationState<A, V>,
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
    state: &ApplicationState<A, V>,
    command: A::Command,
) -> Result<(), AggregateError<<A as Aggregate>::Error>>
where
    A::Command: Send + Sync,
{
    let mut metadata = HashMap::new();
    metadata.insert(
        "timestamp".to_string(),
        time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
    );

    state.execute_with_metadata(&aggregate_id, command, metadata).await
}
