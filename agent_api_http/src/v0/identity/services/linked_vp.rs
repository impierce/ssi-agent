use crate::extractors::RequestActor;
use crate::handlers::{command_handler, internal_command_handler, internal_query_handler};
use agent_identity::{
    document::{aggregate::Status, command::DocumentCommand},
    service::{aggregate::Service, command::ServiceCommand},
    state::{publish_decentrally_hosted_documents, IdentityState},
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
    RequestActor(actor): RequestActor,
    Json(LinkedVPEndpointRequest { presentation_ids }): Json<LinkedVPEndpointRequest>,
) -> Result<Response, ApiError> {
    // Linked VP creation is one installation-wide operation. Its authorization covers discovering
    // eligible DID documents, adding and publishing the service on them, and returning those
    // documents. The fixed follow-up dispatches are trusted continuations of that operation.
    let service_id = "linked-verifiable-presentation-service".to_string();

    let command = ServiceCommand::CreateLinkedVerifiablePresentationService {
        service_id: service_id.clone(),
        presentation_ids,
    };

    // Create a linked verifiable presentation service.
    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &service_id,
        &state.command.service,
        command,
    )
    .await?;

    let linked_verifiable_presentation_service = match internal_query_handler(
        state.authorization_checker.clone(),
        &service_id,
        Some(&service_id),
        &state.query.service,
    )
    .await?
    {
        Some(Service {
            service: Some(linked_verifiable_presentation_service),
            ..
        }) => linked_verifiable_presentation_service,
        // TODO: this *should* be an impossible error, what should we return here?
        _ => return Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)),
    };

    // Query all DID Documents that require an update.
    let document_ids: Vec<String> = internal_query_handler(
        state.authorization_checker.clone(),
        "all_documents",
        None,
        &state.query.all_documents,
    )
    .await?
    .map(|all_documents_view| {
        all_documents_view
            .documents
            .into_values()
            .filter(|document| {
                document.status != Status::Disabled
                    && document
                        .did_method
                        .as_ref()
                        .map(SupportedDidMethod::supports_update)
                        .unwrap_or(false)
            })
            .map(|document| document.document_id)
            .collect()
    })
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))?;

    for document_id in &document_ids {
        let command = DocumentCommand::AddService {
            service_id: service_id.clone(),
            service: Box::new(linked_verifiable_presentation_service.clone()),
        };

        internal_command_handler(
            state.authorization_checker.clone(),
            document_id,
            &state.command.document,
            command,
        )
        .await?;
    }

    publish_decentrally_hosted_documents(&state)
        .await
        .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))?;

    internal_query_handler(
        state.authorization_checker.clone(),
        "all_documents",
        None,
        &state.query.all_documents,
    )
    .await?
    .map(|all_documents_view| {
        let documents: Vec<_> = all_documents_view
            .documents
            .into_values()
            .filter(|document| {
                document.status != Status::Disabled
                    && document
                        .did_method
                        .as_ref()
                        .map(SupportedDidMethod::supports_update)
                        .unwrap_or(false)
            })
            .collect();
        (StatusCode::OK, Json(documents)).into_response()
    })
    // TODO: this *should* be an impossible error, what should we return here?
    .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}
