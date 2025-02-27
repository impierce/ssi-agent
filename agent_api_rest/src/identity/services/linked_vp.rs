use agent_identity::{
    document::{aggregate::Status, command::DocumentCommand},
    service::{aggregate::Service, command::ServiceCommand},
    state::{query_all_documents, IdentityState},
};
use agent_shared::config::SupportedDidMethod;
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedVPEndpointRequest {
    pub presentation_ids: Vec<String>,
}

#[axum_macros::debug_handler]
pub(crate) async fn linked_vp(State(state): State<IdentityState>, Json(payload): Json<Value>) -> Response {
    info!("Request Body: {}", payload);

    let Ok(LinkedVPEndpointRequest { presentation_ids }) = serde_json::from_value(payload) else {
        return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
    };

    let service_id = "linked-verifiable-presentation-service".to_string();
    let command = ServiceCommand::CreateLinkedVerifiablePresentationService {
        service_id: service_id.clone(),
        presentation_ids,
    };

    // Create a linked verifiable presentation service.
    if command_handler(&service_id, &state.command.service, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let linked_verifiable_presentation_service = match query_handler(&service_id, &state.query.service).await {
        Ok(Some(Service {
            service: Some(linked_verifiable_presentation_service),
            ..
        })) => linked_verifiable_presentation_service,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
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
        return StatusCode::PRECONDITION_FAILED.into_response();
    };

    let command = DocumentCommand::AddService {
        service_id,
        service: linked_verifiable_presentation_service,
    };

    if command_handler(&document_id, &state.command.document, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    match query_handler(&document_id, &state.query.document).await {
        Ok(Some(document)) => (StatusCode::OK, Json(document)).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
