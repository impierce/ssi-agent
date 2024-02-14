use crate::state::AggregateHandler;
use cqrs_es::{
    persist::{PersistenceError, ViewRepository},
    Aggregate, AggregateError, View,
};
use std::{collections::HashMap, sync::Arc};
use time::format_description::well_known::Rfc3339;
use tracing::{debug, error};

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

pub async fn command_handler<A>(
    aggregate_id: &str,
    state: &AggregateHandler<A>,
    command: <A as Aggregate>::Command,
) -> Result<(), AggregateError<<A as Aggregate>::Error>>
where
    A: Aggregate,
    <A as Aggregate>::Command: Send + Sync,
{
    let mut metadata = HashMap::new();
    metadata.insert(
        "timestamp".to_string(),
        time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
    );

    state.execute_with_metadata(aggregate_id, command, metadata).await
}
