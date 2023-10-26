use agent_core::{user::Basic as BasicAuth, user::User, Credential};
use axum::{
    extract::{Path, TypedHeader},
    headers::{authorization::Basic, Authorization},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    agent_core::init().await.unwrap();

    let app = Router::new()
        .route("/credential", post(create_credential))
        .route("/credential/:id", get(get_credential))
        .route("/credential/:id/sign", get(sign_credential))
        .route("/events", get(get_all_events));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3033));

    axum::Server::bind(&addr).serve(app.into_make_service()).await.unwrap();
}

#[axum_macros::debug_handler]
async fn create_credential(
    basic_auth: Option<TypedHeader<Authorization<Basic>>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    // TODO: also check against configured basic auth credentials
    if basic_auth.is_none() {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    let id = agent_core::create_credential(None, payload).await;
    (
        StatusCode::CREATED,
        [(header::LOCATION, format!("/credential/{}", id.unwrap()))],
    )
        .into_response()
}

#[axum_macros::debug_handler]
async fn get_credential(Path(id): Path<String>) -> Json<Value> {
    dbg!(&id);
    Json(json!({}))
}

async fn sign_credential(Path(id): Path<String>) -> Json<Value> {
    dbg!(&id);
    let credential = agent_core::sign_credential(id).await.unwrap();
    Json(json!({}))
}

#[axum_macros::debug_handler]
async fn get_all_events() -> Json<Value> {
    let events = agent_core::get_all_credential_events().await.unwrap();
    Json(events)
}
