use agent_issuance::{
    command::IssuanceCommand, handlers::command_handler, model::aggregate::Credential,
    queries::CredentialView,
};
use agent_store::state::ApplicationState;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Router,
};
use serde_json::Value;

pub fn router(state: ApplicationState<Credential, CredentialView>) -> Router {
    Router::new()
        .route("/v1/credentials", post(create_credential_data))
        .with_state(state)
}

// #[axum_macros::debug_handler]
async fn create_credential_data(
    State(state): State<ApplicationState<Credential, CredentialView>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let command = IssuanceCommand::CreateCredentialData {
        credential: payload,
    };

    match command_handler("agg-id-F39A0C".to_string(), state, command).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}
