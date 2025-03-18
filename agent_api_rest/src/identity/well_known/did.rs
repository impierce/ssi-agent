use crate::handlers::query_handler;
use agent_identity::{document::aggregate::Status, state::IdentityState};
use agent_shared::config::SupportedDidMethod;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;

#[axum_macros::debug_handler]
pub(crate) async fn did(State(state): State<IdentityState>) -> Result<Response, ApiError> {
    query_handler("all_documents", &state.query.all_documents)
        .await?
        .and_then(|all_documents_view| {
            all_documents_view.documents.into_values().find_map(|document| {
                document.document.and_then(|core_document| {
                    (document.status != Status::Disabled && document.did_method == Some(SupportedDidMethod::Web))
                        .then_some((StatusCode::OK, Json(core_document)).into_response())
                })
            })
        })
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
