use agent_identity::state::IdentityState;
use agent_shared::{config::SupportedDidMethod, handlers::query_handler};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;

#[axum_macros::debug_handler]
pub(crate) async fn did(State(state): State<IdentityState>) -> Response {
    match query_handler("all_documents", &state.query.all_documents).await {
        Ok(Some(all_documents_view)) => {
            let document = all_documents_view.documents.into_values().find_map(|document| {
                document.document.and_then(|core_document| {
                    (document.did_method == Some(SupportedDidMethod::Web)).then_some(core_document)
                })
            });

            match document {
                Some(document) => (StatusCode::OK, Json(document)).into_response(),
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
