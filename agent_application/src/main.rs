use agent_api_rest::app;
use agent_issuance::{
    queries::SimpleLoggingQuery,
    services::IssuanceServices,
    startup_commands::startup_commands,
    state::{initialize, CQRS},
};
use agent_shared::config;
use agent_store::{in_memory, postgres};

#[tokio::main]
async fn main() {
    let state = match config!("event_store").unwrap().as_str() {
        "postgres" => postgres::ApplicationState::new(vec![Box::new(SimpleLoggingQuery {})], IssuanceServices {}).await,
        _ => in_memory::ApplicationState::new(vec![Box::new(SimpleLoggingQuery {})], IssuanceServices {}).await,
    };

    match config!("log_format").unwrap().as_str() {
        "json" => tracing_subscriber::fmt().json().init(),
        _ => tracing_subscriber::fmt::init(),
    }

    initialize(state.clone(), startup_commands()).await;

    axum::Server::bind(&"0.0.0.0:3033".parse().unwrap())
        .serve(app(state).into_make_service())
        .await
        .unwrap();
}
