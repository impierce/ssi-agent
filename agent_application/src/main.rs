use agent_api_rest::app;
use agent_issuance::{
    credential::services::CredentialServices,
    offer::services::OfferServices,
    // queries::SimpleLoggingQuery,
    server_config::{queries::SimpleLoggingQuery, services::ServerConfigServices},
    // services::IssuanceServices,
    startup_commands::startup_commands_server_config,
    state::{initialize, CQRS},
};
use agent_shared::config;
use agent_store::in_memory;
use lazy_static::lazy_static;

lazy_static! {
    static ref HOST: url::Url = format!("http://{}:3033/", config!("host").unwrap()).parse().unwrap();
}

#[tokio::main]
async fn main() {
    // let state = match config!("event_store").unwrap().as_str() {
    //     // "postgres" => postgres::ApplicationState::new(vec![Box::new(SimpleLoggingQuery {})], IssuanceServices {}).await,
    //     // _ => in_mem::ApplicationState::new(vec![Box::new(SimpleLoggingQuery {})], ServerConfigServices {}).await,
    //     _ => in_memory::ApplicationState::new(vec![Box::new(SimpleLoggingQuery {})], ServerConfigServices {}).await,
    // };

    let credential_state = { in_memory::ApplicationState::new(vec![], CredentialServices).await };
    let offer_state = { in_memory::ApplicationState::new(vec![], OfferServices).await };

    match config!("log_format").unwrap().as_str() {
        "json" => tracing_subscriber::fmt().json().init(),
        _ => tracing_subscriber::fmt::init(),
    }

    // initialize(state.clone(), startup_commands_server_config(HOST.clone())).await;

    axum::Server::bind(&"0.0.0.0:3033".parse().unwrap())
        .serve(app(credential_state, offer_state).into_make_service())
        .await
        .unwrap();
}
