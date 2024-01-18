use crate::{
    credential::{command::CredentialCommand, error::CredentialError},
    offer::{command::OfferCommand, error::OfferError},
    server_config::{command::ServerConfigCommand, error::ServerConfigError},
    state::ApplicationState,
};
use cqrs_es::{persist::PersistenceError, Aggregate, AggregateError, View};
use std::collections::HashMap;
use time::format_description::well_known::Rfc3339;
use tracing::{debug, error};

pub async fn query_handler(
    credential_id: String,
    state: &ApplicationState,
) -> Result<Option<serde_json::Value>, PersistenceError> {
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

pub async fn command_handler_server_config(
    aggregate_id: String,
    state: &ApplicationState,
    command: ServerConfigCommand,
) -> Result<(), AggregateError<ServerConfigError>>
// where
//     A::Command: Send + Sync,
{
    let mut metadata = HashMap::new();
    metadata.insert(
        "timestamp".to_string(),
        time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
    );

    state
        .execute_with_metadata_server_config(&aggregate_id, command, metadata)
        .await
}

pub async fn command_handler_credential(
    aggregate_id: String,
    state: &ApplicationState,
    command: CredentialCommand,
) -> Result<(), AggregateError<CredentialError>> {
    let mut metadata = HashMap::new();
    metadata.insert(
        "timestamp".to_string(),
        time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
    );

    state
        .execute_with_metadata_credential(&aggregate_id, command, metadata)
        .await
}

pub async fn command_handler_offer(
    aggregate_id: String,
    state: &ApplicationState,
    command: OfferCommand,
) -> Result<(), AggregateError<OfferError>> {
    let mut metadata = HashMap::new();

    state
        .execute_with_metadata_offer(&aggregate_id, command, metadata)
        .await
}
