use axum::{routing::get, Router};
use serde_json::json;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let app = Router::new().route("/", get(root));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3033));

    axum::Server::bind(&addr).serve(app.into_make_service()).await.unwrap();
}

async fn root() -> &'static str {
    let credential = agent_core::create_credential(json!({
        "first_name":"Clark",
        "last_name": "Kent",
    }));
    println!("{:?}", credential);
    "Hello, World!"
}
