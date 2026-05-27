use crate::handlers::query_handler;
use agent_library::catalogue::aggregate::Catalogue;
use agent_library::state::LibraryState;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http::StatusCode;
use http_api_problem::ApiError;
use std::sync::Arc;

/// List all catalogues
///
/// List all available catalogues.
#[utoipa::path(
    get,
    path = "/catalogue/get-all-catalogues",
    operation_id = "get_all_catalogues",
    tags = ["Library", "Catalogue"],
    responses(
        (status = 200, description = "All catalogues retrieved successfully", body = [Catalogue]),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_catalogues(State(state): State<Arc<LibraryState>>) -> Result<Response, ApiError> {
    let filtered_catalogues = query_handler("all_catalogues", &state.query.all_catalogues)
        .await?
        .map(|all_catalogues_view| {
            let filtered_catalogues: Vec<Catalogue> = all_catalogues_view
                .catalogues
                .into_values()
                .filter(|catalogue| !catalogue.is_deleted)
                .collect();

            filtered_catalogues
        })
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(filtered_catalogues)).into_response())
}
