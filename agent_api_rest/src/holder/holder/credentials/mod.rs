use agent_holder::{credential::command::CredentialCommand, state::HolderState};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;
use identity_credential::credential::Jwt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::info;

/// Get all credentials
///
/// Retrieve all credentials that this UniCore instance currently holds.
#[utoipa::path(
    get,
    path = "/holder/credentials",
    tag = "Holder",
    responses(
        (status = 200, description = "Successfully retrieved all credentials."),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credentials(State(state): State<HolderState>) -> Response {
    match query_handler("all_holder_credentials", &state.query.all_holder_credentials).await {
        Ok(Some(all_credentials_view)) => {
            let all_credentials = all_credentials_view.credentials.into_values().collect::<Vec<_>>();

            (StatusCode::OK, Json(all_credentials)).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!([]))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HolderCredentialsEndpointRequest {
    pub credential: Jwt,
}

#[axum_macros::debug_handler]
pub(crate) async fn post_credentials(State(state): State<HolderState>, Json(payload): Json<Value>) -> Response {
    info!("Request Body: {}", payload);

    let Ok(HolderCredentialsEndpointRequest { credential }) = serde_json::from_value(payload) else {
        return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
    };

    let holder_credential_id = uuid::Uuid::new_v4().to_string();

    let command = CredentialCommand::AddCredential {
        holder_credential_id: holder_credential_id.clone(),
        received_offer_id: None,
        credential,
    };

    // Add the credential.
    if command_handler(&holder_credential_id, &state.command.credential, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    match query_handler(&holder_credential_id, &state.query.holder_credential).await {
        Ok(Some(holder_credential_view)) => (StatusCode::OK, Json(holder_credential_view)).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[axum_macros::debug_handler]
pub(crate) async fn credential(State(state): State<HolderState>, Path(holder_credential_id): Path<String>) -> Response {
    match query_handler(&holder_credential_id, &state.query.holder_credential).await {
        Ok(Some(holder_credential_view)) => (StatusCode::OK, Json(holder_credential_view)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
