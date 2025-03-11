pub mod presentation_signed;

use crate::handlers::{command_handler, query_handler};
use agent_holder::{
    credential::queries::HolderCredentialView, presentation::command::PresentationCommand, state::HolderState,
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};

#[axum_macros::debug_handler]
pub(crate) async fn get_presentations(State(state): State<HolderState>) -> Result<Response, ApiError> {
    let all_presentations = query_handler("all_presentations", &state.query.all_presentations)
        .await?
        .map(|all_presentations_view| all_presentations_view.presentations.into_values().collect::<Vec<_>>())
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_presentations)).into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn presentation(
    State(state): State<HolderState>,
    Path(presentation_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&presentation_id, &state.query.presentation)
        .await?
        .map(|presentation_view| (StatusCode::CREATED, Json(presentation_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentationsEndpointRequest {
    pub credential_ids: Vec<String>,
}

#[axum_macros::debug_handler]
pub(crate) async fn post_presentations(
    State(state): State<HolderState>,
    Json(payload): Json<PresentationsEndpointRequest>,
) -> Result<Response, ApiError> {
    let mut credentials = vec![];

    // Get all the credentials.
    for credential_id in payload.credential_ids {
        match query_handler(&credential_id, &state.query.holder_credential).await? {
            Some(HolderCredentialView {
                signed: Some(credential),
                ..
            }) => {
                credentials.push(credential);
            }
            _ => return todo!(),
        }
    }

    let presentation_id = uuid::Uuid::new_v4().to_string();

    let command = PresentationCommand::CreatePresentation {
        presentation_id: presentation_id.clone(),
        signed_credentials: credentials,
    };

    // Create the presentation.
    command_handler(&presentation_id, &state.command.presentation, command).await?;

    query_handler(&presentation_id, &state.query.presentation)
        .await?
        .map(|presentation_view| (StatusCode::CREATED, Json(presentation_view)).into_response())
        .ok_or_else(|| {
            ApiError::builder(StatusCode::CONFLICT)
                .title("Optimistic Lock Error")
                .message("An optimistic lock error occurred while committing an aggregate.")
                .finish()
        })
}
