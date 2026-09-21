use agent_holder::{presentation::aggregate::Presentation, state::HolderState};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::{header, StatusCode};
use std::sync::Arc;

use crate::handlers::public_query_handler;

/// Get a signed credential presentation
///
/// Retrieves the compact JWT representation of a stored credential presentation.
#[utoipa::path(
    get,
    path = "/holder/presentations/{presentation_id}/signed",
    operation_id = "get_signed_holder_presentation",
    tags = ["Identity", "Holder"],
    params(
        ("presentation_id" = String, Path, description = "Credential presentation ID"),
    ),
    responses(
        (status = 200, description = "Signed credential presentation", body = String, content_type = "application/jwt"),
        (status = 404, description = "Signed credential presentation not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn presentation_signed(
    State(state): State<Arc<HolderState>>,
    Path(presentation_id): Path<String>,
) -> Result<Response, ApiError> {
    match public_query_handler(&presentation_id, &state.query.presentation).await? {
        Some(Presentation {
            signed: Some(signed_presentation),
            ..
        }) => Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/jwt")],
            signed_presentation.as_str().to_string(),
        )
            .into_response()),
        _ => Err(ApiError::new(StatusCode::NOT_FOUND)),
    }
}
