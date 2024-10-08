use agent_holder::{presentation::aggregate::Presentation, state::HolderState};
use agent_shared::handlers::query_handler;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use hyper::{header, StatusCode};

#[axum_macros::debug_handler]
pub(crate) async fn presentation_signed(
    State(state): State<HolderState>,
    Path(presentation_id): Path<String>,
) -> Response {
    match query_handler(&presentation_id, &state.query.presentation).await {
        Ok(Some(Presentation {
            signed: Some(signed_presentation),
            ..
        })) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/jwt")],
            signed_presentation.as_str().to_string(),
        )
            .into_response(),
        Ok(None) => (StatusCode::NOT_FOUND).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
