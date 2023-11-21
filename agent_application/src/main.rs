use agent_api_rest::app;
use agent_issuance::state::new_application_state;

#[tokio::main]
async fn main() {
    let state = new_application_state().await;
    axum::Server::bind(&"0.0.0.0:3033".parse().unwrap())
        .serve(app(state).into_make_service())
        .await
        .unwrap();
}
