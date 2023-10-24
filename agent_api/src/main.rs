use axum::{routing::get, Json, Router};
use serde_json::{json, Value};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    agent_core::init().await.unwrap();

    let app = Router::new()
        .route("/", get(root))
        .route("/events", get(get_all_events));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3033));

    axum::Server::bind(&addr).serve(app.into_make_service()).await.unwrap();
}

#[axum_macros::debug_handler]
async fn root() -> &'static str {
    let credential = agent_core::create_credential(json!({
        "first_name":"Clark",
        "last_name": "Kent",
    }))
    .await;
    println!("{:?}", credential);
    "SSI Agent at your service!"
}

async fn get_all_events() -> Json<Value> {
    let events = agent_core::get_all_credential_events().await.unwrap();
    Json(events)
}
