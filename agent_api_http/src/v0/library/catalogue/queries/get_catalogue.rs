use crate::handlers::query_handler;
use agent_library::catalogue::aggregate::Catalogue;
use agent_library::state::LibraryState;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http::StatusCode;
use http_api_problem::ApiError;
use std::sync::Arc;

/// Get catalogue by ID
///
/// Retrieve a specific catalogue by its ID.
#[utoipa::path(
    get,
    path = "/catalogue/{catalogue_id}",
    operation_id = "get_catalogue_by_id",
    tags = ["Library", "Catalogue"],
    responses(
        (status = 200, description = "Catalogue retrieved successfully", body = Catalogue)
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_catalogue(
    State(state): State<Arc<LibraryState>>,
    Path(catalogue_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&catalogue_id, &state.query.catalogue)
        .await?
        .and_then(|catalogue_view| {
            if catalogue_view.is_deleted {
                None
            } else {
                Some(catalogue_view)
            }
        })
        .map(|catalogue_view| (StatusCode::OK, Json(Catalogue::from(catalogue_view))).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
