pub mod presentation_signed;

use crate::handlers::{command_handler, load_view, query_handler, request_actor};
use agent_holder::{
    credential::queries::HolderCredentialView, presentation::command::PresentationCommand, state::HolderState,
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Extension, Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Actor;
use std::sync::Arc;

#[axum_macros::debug_handler]
pub(crate) async fn get_presentations(
    State(state): State<Arc<HolderState>>,
    actor: Option<Extension<Option<Actor>>>,
) -> Result<Response, ApiError> {
    let all_presentations = query_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        "all_presentations",
        &state.query.all_presentations,
    )
    .await?
    .map(|all_presentations_view| all_presentations_view.presentations.into_values().collect::<Vec<_>>())
    .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_presentations)).into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn presentation(
    State(state): State<Arc<HolderState>>,
    actor: Option<Extension<Option<Actor>>>,
    Path(presentation_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &presentation_id,
        &state.query.presentation,
    )
    .await?
    .map(|presentation_view| (StatusCode::OK, Json(presentation_view)).into_response())
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentationsEndpointRequest {
    pub credential_ids: Vec<String>,
}

#[axum_macros::debug_handler]
pub(crate) async fn post_presentations(
    State(state): State<Arc<HolderState>>,
    actor: Option<Extension<Option<Actor>>>,
    Json(PresentationsEndpointRequest { credential_ids }): Json<PresentationsEndpointRequest>,
) -> Result<Response, ApiError> {
    let mut credentials = vec![];

    // Get all the credentials.
    for credential_id in credential_ids {
        match load_view(&credential_id, &state.query.holder_credential).await? {
            Some(HolderCredentialView {
                signed: Some(credential),
                ..
            }) => {
                credentials.push(credential);
            }
            _ => return Err(ApiError::new(StatusCode::NOT_FOUND)),
        }
    }

    let presentation_id = uuid::Uuid::new_v4().to_string();

    let command = PresentationCommand::CreatePresentation {
        presentation_id: presentation_id.clone(),
        signed_credentials: credentials,
    };

    // Create the presentation.
    command_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &presentation_id,
        &state.command.presentation,
        command,
    )
    .await?;

    load_view(&presentation_id, &state.query.presentation)
        .await?
        .map(|presentation_view| (StatusCode::CREATED, Json(presentation_view)).into_response())
        // TODO: this *should* be an impossible error, what should we return here?
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}
