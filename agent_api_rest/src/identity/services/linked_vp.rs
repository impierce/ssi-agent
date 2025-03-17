use crate::handlers::{command_handler, query_handler};
use agent_identity::{
    document::{aggregate::Status, command::DocumentCommand},
    service::{aggregate::Service, command::ServiceCommand},
    state::{query_all_documents, IdentityState},
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

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedVPEndpointRequest {
    pub presentation_ids: Vec<String>,
}

#[axum_macros::debug_handler]
pub(crate) async fn linked_vp(
    State(state): State<IdentityState>,
    Json(payload): Json<LinkedVPEndpointRequest>,
) -> Result<Response, ApiError> {
    let service_id = "linked-verifiable-presentation-service".to_string();
    let command = ServiceCommand::CreateLinkedVerifiablePresentationService {
        service_id: service_id.clone(),
        presentation_ids: payload.presentation_ids,
    };

    // Create a linked verifiable presentation service.
    command_handler(&service_id, &state.command.service, command).await?;

    let linked_verifiable_presentation_service = match query_handler(&service_id, &state.query.service).await? {
        Some(Service {
            service: Some(linked_verifiable_presentation_service),
            ..
        }) => linked_verifiable_presentation_service,
        _ => todo!(),
    };

    // Query the DID Web Document to obtain the `document_id`.
    let document_id = if let Some(document_id) = query_all_documents(&state, |(_, document)| {
        document.status != Status::Disabled && document.did_method == Some(SupportedDidMethod::Web)
    })
    .await
    .ok()
    .and_then(|did_web_document| {
        did_web_document
            .keys()
            .next()
            .map(|document_id| document_id.to_string())
    }) {
        document_id
    } else {
        todo!();
    };

    // Query the DID Web Document to obtain the `document_id`.
    let document_id = if let Some(document_id) = query_all_documents(&state, |(_, document)| {
        document.status != Status::Disabled && document.did_method == Some(SupportedDidMethod::Web)
    })
    .await
    .ok()
    .and_then(|did_web_document| {
        did_web_document
            .keys()
            .next()
            .map(|document_id| document_id.to_string())
    }) {
        document_id
    } else {
        todo!();
    };

    let command = DocumentCommand::AddService {
        service_id,
        service: linked_verifiable_presentation_service,
    };

    command_handler(&document_id, &state.command.document, command).await?;

    query_handler(&document_id, &state.query.document)
        .await?
        .map(|document_view| (StatusCode::OK, Json(document_view)).into_response())
        .ok_or_else(|| todo!())
}
