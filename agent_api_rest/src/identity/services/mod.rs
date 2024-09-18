use agent_identity::{document::command::DocumentCommand, service::command::ServiceCommand, state::IdentityState};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use did_manager::DidMethod;
use hyper::StatusCode;

#[axum_macros::debug_handler]
pub(crate) async fn linked_vp(State(state): State<IdentityState>, Path(presentation_id): Path<String>) -> Response {
    let service_id = "linked-verifiable-presentation-service".to_string();
    let command = ServiceCommand::CreateLinkedVerifiablePresentationService {
        service_id: service_id.clone(),
        presentation_id,
    };

    // Create a linked verifiable presentation service.
    command_handler(&service_id, &state.command.service, command)
        .await
        .unwrap();

    let service = query_handler(&service_id, &state.query.service)
        .await
        .unwrap()
        .unwrap()
        .service
        .unwrap();

    let command = DocumentCommand::AddService { service };

    command_handler(&DidMethod::Web.to_string(), &state.command.document, command)
        .await
        .unwrap();

    StatusCode::OK.into_response()
}
