use agent_identity::{document::views::DocumentView, state::IdentityState};
use agent_shared::handlers::query_handler;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use did_manager::DidMethod;
use hyper::StatusCode;

#[axum_macros::debug_handler]
pub(crate) async fn did(State(state): State<IdentityState>) -> Response {
    // TODO: check if enabled
    // Get the DID Document if it exists.
    match query_handler(&DidMethod::Web.to_string(), &state.query.document).await {
        Ok(Some(DocumentView {
            document: Some(document),
            ..
        })) => (StatusCode::OK, Json(document)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
