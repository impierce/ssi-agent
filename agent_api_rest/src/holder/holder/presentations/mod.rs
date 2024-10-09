pub mod presentation_signed;

use agent_holder::{
    credential::queries::HolderCredentialView, presentation::command::PresentationCommand, state::HolderState,
};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::info;

#[axum_macros::debug_handler]
pub(crate) async fn get_presentations(State(state): State<HolderState>) -> Response {
    match query_handler("all_presentations", &state.query.all_presentations).await {
        Ok(Some(all_presentations_view)) => {
            let all_presentations = all_presentations_view
                .presentations
                .into_iter()
                .map(|(_, credential_view)| credential_view)
                .collect::<Vec<_>>();

            (StatusCode::OK, Json(all_presentations)).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!([]))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[axum_macros::debug_handler]
pub(crate) async fn presentation(State(state): State<HolderState>, Path(presentation_id): Path<String>) -> Response {
    match query_handler(&presentation_id, &state.query.presentation).await {
        Ok(Some(presentation_view)) => (StatusCode::OK, Json(presentation_view)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentationsEndpointRequest {
    pub credential_ids: Vec<String>,
}

#[axum_macros::debug_handler]
pub(crate) async fn post_presentations(State(state): State<HolderState>, Json(payload): Json<Value>) -> Response {
    info!("Request Body: {}", payload);

    let Ok(PresentationsEndpointRequest { credential_ids }) = serde_json::from_value(payload) else {
        return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
    };

    let mut credentials = vec![];

    // Get all the credentials.
    for credential_id in credential_ids {
        match query_handler(&credential_id, &state.query.holder_credential).await {
            Ok(Some(HolderCredentialView {
                signed: Some(credential),
                ..
            })) => {
                credentials.push(credential);
            }
            Ok(None) => return StatusCode::NOT_FOUND.into_response(),
            _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }

    let presentation_id = uuid::Uuid::new_v4().to_string();

    let command = PresentationCommand::CreatePresentation {
        presentation_id: presentation_id.clone(),
        signed_credentials: credentials,
    };

    // Create the presentation.
    if command_handler(&presentation_id, &state.command.presentation, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    match query_handler(&presentation_id, &state.query.presentation).await {
        Ok(Some(presentation_view)) => (StatusCode::OK, Json(presentation_view)).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
