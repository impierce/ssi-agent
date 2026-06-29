use crate::handlers::query_handler;
use agent_library::catalog::aggregate::Catalog;
use agent_library::state::LibraryState;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http::StatusCode;
use http_api_problem::ApiError;
use std::sync::Arc;
use agent_library::catalog::views::CatalogView;

/// List all Catalogs
///
/// List all available Catalogs.
#[utoipa::path(
    get,
    path = "/get-all-catalogs",
    operation_id = "get_all_catalogs",
    tags = ["Library", "Catalog"],
    responses(
        (status = 200, description = "All catalogs retrieved successfully", body = [Catalog]),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_all_catalogs(State(state): State<Arc<LibraryState>>) -> Result<Response, ApiError> {
    let filtered_catalogs = query_handler("all_catalogs", &state.query.all_catalogs)
        .await?
        .map(|all_catalogs_view| {
            let filtered_catalogs: Vec<CatalogView> = all_catalogs_view
                .catalogs
                .into_values()
                .filter(|catalog| !catalog.deleted)
                .collect();

            filtered_catalogs
        })
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(filtered_catalogs)).into_response())
}
