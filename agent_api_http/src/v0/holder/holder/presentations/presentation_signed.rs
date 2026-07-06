use agent_holder::{presentation::aggregate::Presentation, state::HolderState};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::{header, StatusCode};
use std::sync::Arc;

use crate::handlers::public_query_handler;

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
