//! Read-side queries of the library.
//!
//! Inbound adapters read single templates and catalogs through these queries instead of the view
//! repositories in [`LibraryState`], so that soft-deleted entities are consistently treated as missing.

use crate::catalog::views::CatalogView;
use crate::state::LibraryState;
use crate::template::views::TemplateView;
use agent_shared::handlers::{live_query_handler, QueryHandlerError};
use shared_kernel::authorization::Caller;

/// Returns the template with the given ID, or `None` if it doesn't exist or has been deleted.
pub async fn get_template(
    state: &LibraryState,
    caller: Caller,
    template_id: &str,
) -> Result<Option<TemplateView>, QueryHandlerError> {
    live_query_handler(
        state.authorization_checker.as_ref(),
        caller,
        template_id,
        Some(template_id),
        state.query.template.as_ref(),
    )
    .await
}

/// Returns the catalog with the given ID, or `None` if it doesn't exist or has been deleted.
pub async fn get_catalog(
    state: &LibraryState,
    caller: Caller,
    catalog_id: &str,
) -> Result<Option<CatalogView>, QueryHandlerError> {
    live_query_handler(
        state.authorization_checker.as_ref(),
        caller,
        catalog_id,
        Some(catalog_id),
        state.query.catalog.as_ref(),
    )
    .await
}
