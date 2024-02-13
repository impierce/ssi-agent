use crate::state::AggregateHandler;
use cqrs_es::{persist::PersistenceError, Aggregate, AggregateError, View};
use std::collections::HashMap;
use time::format_description::well_known::Rfc3339;
use tracing::{debug, error};

pub async fn query_handler<A, V>(
    credential_id: String,
    state: &AggregateHandler<A, V>,
) -> Result<Option<V>, PersistenceError>
where
    A: Aggregate,
    V: View<A>,
{
    match state.load(&credential_id).await {
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

pub async fn command_handler<A, V>(
    aggregate_id: String,
    state: &AggregateHandler<A, V>,
    command: <A as Aggregate>::Command,
) -> Result<(), AggregateError<<A as Aggregate>::Error>>
where
    A: Aggregate,
    V: View<A>,
    <A as Aggregate>::Command: Send + Sync,
{
    let mut metadata = HashMap::new();
    metadata.insert(
        "timestamp".to_string(),
        time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
    );

    state.execute_with_metadata(&aggregate_id, command, metadata).await
}
