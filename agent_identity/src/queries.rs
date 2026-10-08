//! Read-side queries of the identity module.
//!
//! Inbound adapters read connections and services through these queries instead of the view
//! repositories in [`IdentityState`], so that removed entities are consistently treated as missing.

use crate::connection::views::ConnectionView;
use crate::service::views::{all_services::AllServicesView, ServiceView};
use crate::state::IdentityState;
use agent_shared::handlers::{live_query_handler, query_handler, QueryHandlerError};
use shared_kernel::authorization::Caller;
use shared_kernel::view_repository::SoftDeletable;

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

/// Returns the service with the given ID, or `None` if it doesn't exist or has been deleted.
pub async fn get_service(
    state: &IdentityState,
    caller: Caller,
    service_id: &str,
) -> Result<Option<ServiceView>, QueryHandlerError> {
    live_query_handler(
        state.authorization_checker.as_ref(),
        caller,
        service_id,
        Some(service_id),
        state.query.service.as_ref(),
    )
    .await
}

/// Returns all services that haven't been deleted.
///
/// Unlike the other list projections, `AllServicesView` keeps deleted services, because a deleted
/// service can be revived by later events (see ADR 0007).
pub async fn list_services(state: &IdentityState, caller: Caller) -> Result<AllServicesView, QueryHandlerError> {
    let mut all_services = query_handler(
        state.authorization_checker.clone(),
        caller,
        "all_services",
        None,
        &state.query.all_services,
    )
    .await?
    .unwrap_or_default();
    all_services.services.retain(|_, service| !service.is_deleted());

    Ok(all_services)
}
