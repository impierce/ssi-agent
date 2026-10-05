//! Read-side queries of the identity module.
//!
//! Inbound adapters read single connections through these queries instead of the view repositories
//! in [`IdentityState`], so that removed connections are consistently treated as missing.

use crate::connection::views::ConnectionView;
use crate::state::IdentityState;
use agent_shared::handlers::{live_query_handler, QueryHandlerError};
use shared_kernel::authorization::Caller;

/// Returns the connection with the given ID, or `None` if it doesn't exist or has been removed.
pub async fn get_connection(
    state: &IdentityState,
    caller: Caller,
    connection_id: &str,
) -> Result<Option<ConnectionView>, QueryHandlerError> {
    live_query_handler(
        state.authorization_checker.as_ref(),
        caller,
        connection_id,
        Some(connection_id),
        state.query.connection.as_ref(),
    )
    .await
}
