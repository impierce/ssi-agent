use crate::handlers::{command_handler, query_handler};
use agent_identity::{
    document::{aggregate::Status, command::DocumentCommand},
    service::{aggregate::Service, command::ServiceCommand},
    state::IdentityState,
};
use agent_shared::config::SupportedDidMethod;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedVPEndpointRequest {
    pub presentation_ids: Vec<String>,
}

#[axum_macros::debug_handler]
pub(crate) async fn linked_vp(
    State(state): State<Arc<IdentityState>>,
    Json(LinkedVPEndpointRequest { presentation_ids }): Json<LinkedVPEndpointRequest>,
) -> Result<Response, ApiError> {
    let service_id = "linked-verifiable-presentation-service".to_string();

    let command = ServiceCommand::CreateLinkedVerifiablePresentationService {
        service_id: service_id.clone(),
        presentation_ids,
    };

    // Create a linked verifiable presentation service.
    command_handler(&service_id, &state.command.service, command).await?;

    let linked_verifiable_presentation_service = match query_handler(&service_id, &state.query.service).await? {
        Some(Service {
            service: Some(linked_verifiable_presentation_service),
            ..
        }) => linked_verifiable_presentation_service,
        // TODO: this *should* be an impossible error, what should we return here?
        _ => return Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)),
    };

    // Query the DID Web Document to obtain the `document_id`.
    let document_id = query_handler("all_documents", &state.query.all_documents)
        .await?
        .and_then(|all_documents_view| {
            all_documents_view.documents.into_values().find_map(|document| {
                (document.status != Status::Disabled && document.did_method == Some(SupportedDidMethod::Web))
                    .then_some(document.document_id)
            })
        })
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))?;

    let command = DocumentCommand::AddService {
        service_id,
        service: Box::new(linked_verifiable_presentation_service),
    };

    command_handler(&document_id, &state.command.document, command).await?;

    query_handler(&document_id, &state.query.document)
        .await?
        .map(|document_view| (StatusCode::OK, Json(document_view)).into_response())
        // TODO: this *should* be an impossible error, what should we return here?
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}
