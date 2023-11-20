use agent_issuance::{
    handlers::{command_handler, query_handler},
    state::new_application_state,
};
use axum::{routing::get, Router};

#[tokio::main]
async fn main() {
    let state = new_application_state().await;
    // Configure the Axum routes and services.
    // For this example a single logical endpoint is used and the HTTP method
    // distinguishes whether the call is a command or a query.
    let router = Router::new()
        .route(
            "/credential/:credential_id",
            get(query_handler).post(command_handler),
        )
        // .route("/actual_credential/:credential_id", post(command_handler))
        .with_state(state);
    // Start the Axum server.
    axum::Server::bind(&"0.0.0.0:3030".parse().unwrap())
        .serve(router.into_make_service())
        .await
        .unwrap();
}
