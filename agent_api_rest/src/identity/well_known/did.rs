use agent_identity::{document::views::DocumentView, state::IdentityState};
use agent_shared::{config::SupportedDidMethod, handlers::query_handler};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;

#[axum_macros::debug_handler]
pub(crate) async fn did(State(state): State<IdentityState>) -> Response {
    match query_handler(&SupportedDidMethod::Web.to_string(), &state.query.document).await {
        Ok(Some(DocumentView {
            document: Some(document),
            ..
        })) => (StatusCode::OK, Json(document)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
