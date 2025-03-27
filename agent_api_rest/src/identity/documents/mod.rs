use crate::handlers::query_handler;
use agent_identity::state::IdentityState;
use agent_shared::config::SupportedDidMethod;
use axum::{
    extract::{Form, Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Deserialize, Serialize)]
pub struct GetDocumentsEndpoint {
    #[serde(default)]
    pub did_method: Option<SupportedDidMethod>,
}

pub(crate) async fn get_documents(
    State(state): State<IdentityState>,
    Form(GetDocumentsEndpoint { did_method }): Form<GetDocumentsEndpoint>,
) -> Result<Response, ApiError> {
    debug!("Request Params - did_method: {did_method:?}");

    let filtered_documents = query_handler("all_documents", &state.query.all_documents)
        .await?
        .map(|all_documents_view| {
            let filtered_documents: Vec<_> = all_documents_view
                .documents
                .into_values()
                .filter(|document| {
                    did_method
                        .as_ref()
                        .map_or(true, |method| document.did_method.as_ref() == Some(method))
                })
                .collect();

            filtered_documents
        })
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(filtered_documents)).into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn get_document(
    State(state): State<IdentityState>,
    Path(document_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&document_id, &state.query.document)
        .await?
        .map(|document_view| (StatusCode::OK, Json(document_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
