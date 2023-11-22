use agent_issuance::{
    command::IssuanceCommand,
    handlers::{command_handler, query_handler},
    model::aggregate::IssuanceData,
    queries::IssuanceDataView,
};
use agent_store::state::ApplicationState;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Form, Router,
};
use oid4vci::{credential_request::CredentialRequest, token_request::TokenRequest};
use serde_json::Value;

// TODO: What to do with aggregate_id's?
const AGGREGATE_ID: &str = "agg-id-F39A0C";

pub fn router(state: ApplicationState<IssuanceData, IssuanceDataView>) -> Router {
    Router::new()
        .route("/v1/credentials", post(create_unsigned_credential))
        .route(
            "/v1/openid4vci/.well-known/oauth-authorization-server",
            get(oauth_authorization_server),
        )
        .route(
            "/v1/openid4vci/.well-known/openid_credential_issuer",
            get(openid_credential_issuer),
        )
        .route("/v1/openid4vci/token", post(token))
        .route("/v1/openid4vci/credential", post(credential))
        .with_state(state)
}

#[axum_macros::debug_handler]
async fn create_unsigned_credential(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let command = IssuanceCommand::CreateUnsignedCredential {
        unsigned_credential: payload,
    };

    match command_handler(AGGREGATE_ID.to_string(), &state, command).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[axum_macros::debug_handler]
async fn oauth_authorization_server(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
) -> impl IntoResponse {
    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) => (StatusCode::OK, Json(view.oid4vci_data.authorization_server_metadata)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[axum_macros::debug_handler]
async fn openid_credential_issuer(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
) -> impl IntoResponse {
    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) => (StatusCode::OK, Json(view.oid4vci_data.credential_issuer_metadata)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[axum_macros::debug_handler]
async fn token(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
    Form(token_request): Form<TokenRequest>,
) -> impl IntoResponse {
    let command = IssuanceCommand::CreateTokenResponse { token_request };

    match command_handler(AGGREGATE_ID.to_string(), &state, command).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    };

    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) => (StatusCode::OK, Json(view.subjects[0].token_response.clone())).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[axum_macros::debug_handler]
async fn credential(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
    // TODO: add AuthBearer(access_token): AuthBearer,
    Json(credential_request): Json<CredentialRequest>,
) -> impl IntoResponse {
    let command = IssuanceCommand::CreateCredentialResponse { credential_request };

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
