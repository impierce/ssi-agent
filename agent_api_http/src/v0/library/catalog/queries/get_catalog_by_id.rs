use crate::handlers::query_handler;
use crate::v0::library::catalog::CatalogDto;
use agent_library::state::LibraryState;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http::StatusCode;
use http_api_problem::ApiError;
use std::sync::Arc;

/// Get catalog by ID
///
/// Retrieve a specific catalog by its ID.
#[utoipa::path(
    get,
    path = "/get-catalog-by-id/{catalog_id}",
    operation_id = "get_catalog_by_id",
    tags = ["Library", "Catalog"],
    responses(
        (status = 200, description = "Catalog retrieved successfully", body = CatalogDto),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_catalog_by_id(
    State(state): State<Arc<LibraryState>>,
    Path(catalog_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&catalog_id, &state.query.catalog)
        .await?
        .and_then(|catalog_view| (!catalog_view.deleted).then_some(catalog_view))
        .map(|catalog_view| (StatusCode::OK, Json(CatalogDto::from(catalog_view))).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
