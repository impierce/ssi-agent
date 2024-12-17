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
use utoipa::ToSchema;

/// List all Presentations
///
/// Retrieve all presentations that this UniCore instance currently holds.
/// A Presentation contains one or more Credentials.
#[utoipa::path(
    get,
    path = "/holder/presentations",
    tag = "Holder",
    responses(
        (status = 200, description = "Successfully retrieved all presentations."),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_presentations(State(state): State<HolderState>) -> Response {
    match query_handler("all_presentations", &state.query.all_presentations).await {
        Ok(Some(all_presentations_view)) => {
            let all_presentations = all_presentations_view.presentations.into_values().collect::<Vec<_>>();

            (StatusCode::OK, Json(all_presentations)).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!([]))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Get a Presentation by ID
///
/// Retrieves a presentation for a given ID.
/// A Presentation contains one or more Credentials.
#[utoipa::path(
    get,
    path = "/holder/presentations/{id}",
    params(
        ("id" = String, Path, description = "Unique identifier of the Presentation", example = "57ea9bf4-3a50-4b34-a340-7ef969bfab12"),
    ),
    tag = "Holder",
    responses(
        (status = 200, description = "Successfully retrieved the presentation."),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn presentation(State(state): State<HolderState>, Path(presentation_id): Path<String>) -> Response {
    match query_handler(&presentation_id, &state.query.presentation).await {
        Ok(Some(presentation_view)) => (StatusCode::OK, Json(presentation_view)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PresentationsEndpointRequest {
    pub credential_ids: Vec<String>,
}

/// Create new Presentation for given Credentials
///
/// One or more Credentials in UniCore's Holder wallet can be made available to be verified by other parties.
/// Depending on the content of the Credentials, this can increase the trustworthiness of this UniCore instance.
#[utoipa::path(
    post,
    path = "/holder/presentations",
    request_body = PresentationsEndpointRequest,
    tag = "Holder",
    responses(
        (status = 200, description = "Successfully created a presentation."),
    )
)]
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
