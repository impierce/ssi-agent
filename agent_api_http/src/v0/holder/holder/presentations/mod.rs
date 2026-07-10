pub mod presentation_signed;

use crate::extractors::RequestActor;
use crate::handlers::{command_handler, query_handler};
use agent_holder::{
    credential::queries::HolderCredentialView,
    presentation::{aggregate::Presentation, command::PresentationCommand},
    state::HolderState,
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// List all credential presentations
///
/// Retrieves all credential presentations held by your organisation.
#[utoipa::path(
    get,
    path = "/holder/presentations",
    operation_id = "get_all_holder_presentations",
    tags = ["Identity", "Holder"],
    responses(
        (status = 200, description = "All presentations retrieved successfully", body = [Presentation]),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_presentations(
    State(state): State<Arc<HolderState>>,
    RequestActor(actor): RequestActor,
) -> Result<Response, ApiError> {
    let all_presentations = query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        "all_presentations",
        &state.query.all_presentations,
    )
    .await?
    .map(|all_presentations_view| all_presentations_view.presentations.into_values().collect::<Vec<_>>())
    .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_presentations)).into_response())
}

/// Get credential presentation by ID
///
/// Retrieves a single credential presentation held by your organisation by its ID.
#[utoipa::path(
    get,
    path = "/holder/presentations/{presentation_id}",
    operation_id = "get_holder_presentation_by_id",
    tags = ["Identity", "Holder"],
    responses(
        (status = 200, description = "Presentation retrieved successfully", body = Presentation),
        (status = 404, description = "Presentation not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn presentation(
    State(state): State<Arc<HolderState>>,
    RequestActor(actor): RequestActor,
    Path(presentation_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &presentation_id,
        &state.query.presentation,
    )
    .await?
    .map(|presentation_view| (StatusCode::OK, Json(presentation_view)).into_response())
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PresentationsEndpointRequest {
    pub credential_ids: Vec<String>,
}

/// Create a new credential presentation
///
/// Creates and signs a new credential presentation containing the given credentials held by your organisation.
#[utoipa::path(
    post,
    path = "/holder/presentations",
    operation_id = "create_holder_presentation",
    tags = ["Identity", "Holder"],
    request_body = PresentationsEndpointRequest,
    responses(
        (status = 201, description = "Presentation created successfully", body = Presentation),
        (status = 404, description = "Credential not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn post_presentations(
    State(state): State<Arc<HolderState>>,
    RequestActor(actor): RequestActor,
    Json(PresentationsEndpointRequest { credential_ids }): Json<PresentationsEndpointRequest>,
) -> Result<Response, ApiError> {
    let mut credentials = vec![];

    // Get all the credentials.
    for credential_id in credential_ids {
        match query_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &credential_id,
            &state.query.holder_credential,
        )
        .await?
        {
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
        actor.clone(),
        &presentation_id,
        &state.command.presentation,
        command,
    )
    .await?;

    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &presentation_id,
        &state.query.presentation,
    )
    .await?
    .map(|presentation_view| (StatusCode::CREATED, Json(presentation_view)).into_response())
    // TODO: this *should* be an impossible error, what should we return here?
    .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}
