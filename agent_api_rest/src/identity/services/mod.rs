use agent_identity::{
    document::command::DocumentCommand,
    service::{aggregate::Service, command::ServiceCommand},
    state::IdentityState,
};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use did_manager::DidMethod;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedVPEndpointRequest {
    pub presentation_id: String,
}

#[axum_macros::debug_handler]
pub(crate) async fn linked_vp(State(state): State<IdentityState>, Json(payload): Json<Value>) -> Response {
    info!("Request Body: {}", payload);

    let Ok(LinkedVPEndpointRequest { presentation_id }) = serde_json::from_value(payload) else {
        return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
    };

    let service_id = "linked-verifiable-presentation-service".to_string();
    let command = ServiceCommand::CreateLinkedVerifiablePresentationService {
        service_id: service_id.clone(),
        presentation_id,
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

    let command = DocumentCommand::AddService {
        service: linked_verifiable_presentation_service,
    };

    if command_handler(&DidMethod::Web.to_string(), &state.command.document, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    match query_handler(&DidMethod::Web.to_string(), &state.query.document).await {
        Ok(Some(document)) => (StatusCode::OK, Json(document)).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
