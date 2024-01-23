use crate::{
    credential::{command::CredentialCommand, error::CredentialError},
    offer::{command::OfferCommand, error::OfferError},
    server_config::{command::ServerConfigCommand, error::ServerConfigError},
    state::AggregateHandler,
};
use cqrs_es::{persist::PersistenceError, Aggregate, AggregateError, View};
use serde_json::Value;
use std::collections::HashMap;
use time::format_description::well_known::Rfc3339;
use tracing::{debug, error};

pub async fn query_handler<A: Aggregate, V: View<A>>(
    credential_id: String,
    state: &AggregateHandler<A, V>,
) -> Result<Option<V>, PersistenceError> {
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

pub async fn command_handler<A: Aggregate, V: View<A>>(
    aggregate_id: String,
    state: &AggregateHandler<A, V>,
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

// pub async fn command_handler_server_config(
//     aggregate_id: String,
//     state: &ApplicationState,
//     command: ServerConfigCommand,
// ) -> Result<(), AggregateError<ServerConfigError>>
// // where
// //     A::Command: Send + Sync,
// {
//     let mut metadata = HashMap::new();
//     metadata.insert(
//         "timestamp".to_string(),
//         time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
//     );

//     state
//         .execute_with_metadata_server_config(&aggregate_id, command, metadata)
//         .await
// }

// pub async fn command_handler_credential(
//     aggregate_id: String,
//     state: &ApplicationState,
//     command: CredentialCommand,
// ) -> Result<(), AggregateError<CredentialError>> {
//     let mut metadata = HashMap::new();
//     metadata.insert(
//         "timestamp".to_string(),
//         time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
//     );

//     todo!()

//     // state
//     //     .execute_with_metadata_credential(&aggregate_id, command, metadata)
//     //     .await
// }

// pub async fn command_handler_offer(
//     aggregate_id: String,
//     state: &ApplicationState,
//     command: OfferCommand,
// ) -> Result<(), AggregateError<OfferError>> {
//     let mut metadata = HashMap::new();

//     metadata.insert(
//         "timestamp".to_string(),
//         time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
//     );

//     todo!()

//     // state
//     //     .execute_with_metadata_offer(&aggregate_id, command, metadata)
//     //     .await
// }
