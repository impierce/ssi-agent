use agent_shared::{
    config::{BITS_PER_STATUS, STATUS_LIST_BYTES_AMOUNT},
    handlers::{command_handler, public_query_handler, CommandHandlerError},
};
use cqrs_es::persist::PersistenceError;
use identity_kit::status_list::StatusType;
use shared_kernel::authorization::Caller;

use crate::{
    state::IssuanceState,
    status_list::{command::StatusListCommand, error::StatusListError},
};

#[cfg(feature = "test_utils")]
use agent_shared::config::TEST_STATUS_LIST_ID;

#[derive(Debug, thiserror::Error)]
pub enum IndexAllocationError {
    #[error(transparent)]
    Command(#[from] CommandHandlerError<StatusListError>),
    #[error(transparent)]
    Query(#[from] PersistenceError),
    #[error("Status list `{0}` has no index after one was added")]
    IndexMissing(String),
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
pub async fn add_index(state: &IssuanceState, status_list_id: &str) -> Result<usize, IndexAllocationError> {
    let command = StatusListCommand::AddIndex {
        status: StatusType::VALID,
    };
    command_handler(
        state.authorization_checker.clone(),
        Caller::Internal,
        status_list_id,
        &state.command.status_list,
        command,
    )
    .await?;

    // TODO: even though the AddIndex command is executed right before this, retrieving the index this way is not the
    // prettiest since something of a "data race" could occur where another index has already been added between the
    // AddIndex command and this command. Then two credentials would be assigned the same index, as they both retrieve
    // the same last index.
    public_query_handler(status_list_id, &state.query.status_list)
        .await?
        .and_then(|status_list| status_list.used_indices.last().cloned())
        .ok_or_else(|| IndexAllocationError::IndexMissing(status_list_id.to_owned()))
}
