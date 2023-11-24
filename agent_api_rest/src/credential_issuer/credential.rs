use agent_issuance::{
    command::IssuanceCommand,
    handlers::{command_handler, query_handler},
    model::aggregate::IssuanceData,
    queries::IssuanceDataView,
    state::ApplicationState,
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use axum_auth::AuthBearer;
use oid4vci::credential_request::CredentialRequest;

use crate::AGGREGATE_ID;

#[axum_macros::debug_handler]
pub(crate) async fn credential(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
    AuthBearer(access_token): AuthBearer,
    Json(credential_request): Json<CredentialRequest>,
) -> impl IntoResponse {
    let command = IssuanceCommand::CreateCredentialResponse {
        access_token,
        credential_request,
    };

    match command_handler(AGGREGATE_ID.to_string(), &state, command).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    };

    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) => (StatusCode::OK, Json(view.subjects[0].credential_response.clone())).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}
