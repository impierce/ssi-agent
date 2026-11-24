use agent_shared::{
    config::{BITS_PER_STATUS, STATUS_LIST_BYTES_AMOUNT},
    handlers::{command_handler, public_query_handler, CommandHandlerError},
};
use cqrs_es::{persist::PersistenceError, AggregateError};
use identity_kit::status_list::StatusType;
use shared_kernel::authorization::Caller;

use crate::{
    state::IssuanceState,
    status_list::{command::StatusListCommand, error::StatusListError},
};

#[cfg(feature = "test_utils")]
use agent_shared::config::TEST_STATUS_LIST_ID;

/// How often an index is chosen again when another one was added to the status list at the same time.
const ATTEMPTS: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum IndexAllocationError {
    #[error(transparent)]
    Command(#[from] CommandHandlerError<StatusListError>),
    #[error(transparent)]
    Query(#[from] PersistenceError),
    #[error("Status list `{0}` not found")]
    StatusListNotFound(String),
    #[error("Status list `{0}` has no unused index")]
    StatusListFull(String),
}

/// Returns a status list with room for `count` more indices, creating one when none has.
///
/// A status list is filled to at most 70%: the remainder is reserved for random values that protect
/// the privacy of Issuer and Holder.
pub async fn status_list_with_room(state: &IssuanceState, count: usize) -> Result<String, IndexAllocationError> {
    let all_status_lists = public_query_handler("all_status_lists", &state.query.all_status_lists)
        .await?
        .map(|all_status_lists_view| all_status_lists_view.status_lists.into_values().collect::<Vec<_>>())
        .unwrap_or_default();

    let max_amount_indices = STATUS_LIST_BYTES_AMOUNT * 8 / BITS_PER_STATUS as usize;
    let available_status_list = all_status_lists
        .into_iter()
        .find(|status_list| status_list.used_indices.len() + count <= (max_amount_indices * 7) / 10);

    if let Some(status_list) = available_status_list {
        return Ok(status_list.id);
    }

    #[cfg(not(feature = "test_utils"))]
    let id = uuid::Uuid::new_v4().to_string();

    #[cfg(feature = "test_utils")]
    let id = TEST_STATUS_LIST_ID.to_string();

    let command = StatusListCommand::CreateStatusList { id: id.clone() };
    command_handler(
        state.authorization_checker.clone(),
        Caller::Internal,
        &id,
        &state.command.status_list,
        command,
    )
    .await?;

    Ok(id)
}

/// Adds a `VALID` index to the status list and returns it.
///
/// The index is chosen here and the status list refuses one it has used, so each caller knows the index it added.
/// When another caller adds the same index, or any index, at the same time, the index is chosen again.
pub async fn add_index(state: &IssuanceState, status_list_id: &str) -> Result<usize, IndexAllocationError> {
    let mut attempt = 1;
    loop {
        let index = public_query_handler(status_list_id, &state.query.status_list)
            .await?
            .ok_or_else(|| IndexAllocationError::StatusListNotFound(status_list_id.to_owned()))?
            .unused_index()
            .ok_or_else(|| IndexAllocationError::StatusListFull(status_list_id.to_owned()))?;

        let command = StatusListCommand::AddIndex {
            index,
            status: StatusType::VALID,
        };
        match command_handler(
            state.authorization_checker.clone(),
            Caller::Internal,
            status_list_id,
            &state.command.status_list,
            command,
        )
        .await
        {
            Ok(()) => return Ok(index),
            Err(CommandHandlerError::Aggregate(
                AggregateError::AggregateConflict | AggregateError::UserError(StatusListError::IndexAlreadyUsed(_)),
            )) if attempt < ATTEMPTS => attempt += 1,
            Err(err) => return Err(err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{services::IssuanceServices, state::issuance_state};
    use agent_secret_manager::service::Service;
    use agent_shared::config::TESTINDEX;
    use agent_store::in_memory::InMemory;

    #[tokio::test]
    async fn each_allocation_returns_the_index_it_added() {
        let state = issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await;
        let status_list_id = status_list_with_room(&state, 3).await.unwrap();

        let mut indices = vec![];
        for _ in 0..3 {
            indices.push(add_index(&state, &status_list_id).await.unwrap());
        }

        assert_eq!(indices, vec![TESTINDEX, TESTINDEX + 1, TESTINDEX + 2]);
        let status_list = public_query_handler(&status_list_id, &state.query.status_list)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status_list.used_indices, indices);
    }

    #[tokio::test]
    async fn an_unknown_status_list_has_no_index_to_add() {
        let state = issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await;

        assert!(matches!(
            add_index(&state, "unknown").await,
            Err(IndexAllocationError::StatusListNotFound(_))
        ));
    }
}
