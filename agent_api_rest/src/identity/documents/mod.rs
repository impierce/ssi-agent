use agent_identity::state::IdentityState;
use agent_shared::config::SupportedDidMethod;
use agent_shared::handlers::query_handler;
use axum::{
    extract::{Form, Path, State},
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::debug;

#[derive(Deserialize, Serialize)]
pub struct GetDocumentsEndpoint {
    #[serde(default)]
    pub did_method: Option<SupportedDidMethod>,
}

pub(crate) async fn get_documents(
    State(state): State<IdentityState>,
    Form(GetDocumentsEndpoint { did_method }): Form<GetDocumentsEndpoint>,
) -> Response {
    debug!("Request Params - did_method: {did_method:?}");

    match query_handler("all_documents", &state.query.all_documents).await {
        Ok(Some(all_documents_view)) => {
            let filtered_documents: Vec<_> = all_documents_view
                .documents
                .values()
                .filter(|document| {
                    did_method
                        .as_ref()
                        .map_or(true, |method| document.did_method.as_ref() == Some(method))
                })
                .collect();

            (StatusCode::OK, Json(filtered_documents)).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!([]))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[axum_macros::debug_handler]
pub(crate) async fn get_document(State(state): State<IdentityState>, Path(document_id): Path<String>) -> Response {
    match query_handler(&document_id, &state.query.document).await {
        Ok(Some(document)) => (StatusCode::OK, Json(document)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
