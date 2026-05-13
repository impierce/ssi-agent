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
use std::sync::Arc;

#[axum_macros::debug_handler]
pub(crate) async fn get_presentations(State(state): State<Arc<HolderState>>) -> Result<Response, ApiError> {
    let all_presentations = query_handler("all_presentations", &state.query.all_presentations)
        .await?
        .map(|all_presentations_view| all_presentations_view.presentations.into_values().collect::<Vec<_>>())
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_presentations)).into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn presentation(
    State(state): State<Arc<HolderState>>,
    Path(presentation_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&presentation_id, &state.query.presentation)
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
    Json(PresentationsEndpointRequest { credential_ids }): Json<PresentationsEndpointRequest>,
) -> Result<Response, ApiError> {
    let mut credentials = vec![];

    // Get all the credentials.
    for credential_id in credential_ids {
        match query_handler(&credential_id, &state.query.holder_credential).await? {
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
        None,
        &presentation_id,
        &state.command.presentation,
        command,
    )
    .await?;

    query_handler(&presentation_id, &state.query.presentation)
        .await?
        .map(|presentation_view| (StatusCode::CREATED, Json(presentation_view)).into_response())
        // TODO: this *should* be an impossible error, what should we return here?
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v0::holder::holder::credentials::{post_credentials, HolderCredentialsEndpointRequest};
    use agent_holder::services::HolderServices;
    use agent_issuance::credential::aggregate::test_utils::JWT_VC_JSON_OBV3_JWT;
    use agent_secret_manager::service::Service;
    use agent_store::{holder_state, in_memory::InMemory};
    use axum::body;
    use identity_credential::credential::Jwt;

    #[tokio::test]
    async fn post_presentations_dispatches_create_presentation_command() {
        let state = Arc::new(holder_state(&InMemory, HolderServices::default().await, Default::default()).await);
        let response = post_credentials(
            State(state.clone()),
            Json(HolderCredentialsEndpointRequest {
                credential: Jwt::from(JWT_VC_JSON_OBV3_JWT.to_string()),
            }),
        )
        .await
        .unwrap();
        let body = body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let credential: HolderCredentialView = serde_json::from_slice(&body).unwrap();

        let response = post_presentations(
            State(state),
            Json(PresentationsEndpointRequest {
                credential_ids: vec![credential.holder_credential_id],
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
    }
}
