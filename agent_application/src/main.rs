use std::env;

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

    if let Ok(val) = config!("log_format") {
        if &val == "json" {
            tracing_subscriber::fmt().json().init()
        } else {
            tracing_subscriber::fmt::init()
        }
    } else {
        tracing_subscriber::fmt::init()
    }

    let url = env::var_os("AGENT_APPLICATION_URL")
        .expect("AGENT_APPLICATION_URL is not set")
        .into_string()
        .unwrap();

    tracing::info!("Application url: {:?}", url);

    let url = url::Url::parse(&url).unwrap();

    initialize(state.clone(), startup_commands(url)).await;

    let server = "0.0.0.0:3033".parse().unwrap();

    axum::Server::bind(&server)
        .serve(app(state.clone()).into_make_service())
        .await
        .unwrap();
}
