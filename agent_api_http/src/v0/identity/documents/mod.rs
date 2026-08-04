use crate::extractors::RequestActor;
use crate::handlers::query_handler;
use agent_identity::{document::aggregate::Document, state::IdentityState};
use agent_shared::config::SupportedDidMethod;
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::debug;

#[derive(Deserialize, Serialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct GetDocumentsEndpoint {
    #[serde(default)]
    pub did_method: Option<SupportedDidMethod>,
}

/// List DID documents
///
/// Retrieves the list of DID documents in use by your organisation.
#[utoipa::path(
    get,
    path = "/documents",
    operation_id = "get_all_documents",
    tags = ["Identity"],
    params(GetDocumentsEndpoint),
    responses(
        (status = 200, description = "Documents retrieved successfully", body = [Document]),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_documents(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Query(GetDocumentsEndpoint { did_method }): Query<GetDocumentsEndpoint>,
) -> Result<Response, ApiError> {
    debug!("Request Params - did_method: {did_method:?}");

    let filtered_documents = query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        "all_documents",
        None,
        &state.query.all_documents,
    )
    .await?
    .map(|all_documents_view| {
        let filtered_documents: Vec<_> = all_documents_view
            .documents
            .into_values()
            .filter(|document| {
                did_method
                    .as_ref()
                    .is_none_or(|method| document.did_method.as_ref() == Some(method))
            })
            .collect();

        filtered_documents
    })
    .unwrap_or_default();

    Ok((StatusCode::OK, Json(filtered_documents)).into_response())
}

/// Get DID document by ID
///
/// Retrieves a DID document of your organisation by its ID.
#[utoipa::path(
    get,
    path = "/documents/{document_id}",
    operation_id = "get_document_by_id",
    tags = ["Identity"],
    responses(
        (status = 200, description = "Document retrieved successfully", body = Document),
        (status = 404, description = "Document not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_document(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Path(document_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &document_id,
        Some(&document_id),
        &state.query.document,
    )
    .await?
    .map(|document_view| (StatusCode::OK, Json(document_view)).into_response())
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
