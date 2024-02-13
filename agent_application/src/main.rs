use agent_api_rest::app;
use agent_issuance::{startup_commands::startup_commands_server_config, state::initialize};
use agent_shared::config;
use agent_store::{in_memory, postgres};
use lazy_static::lazy_static;

lazy_static! {
    static ref HOST: url::Url = format!("http://{}:3033/", config!("host").unwrap()).parse().unwrap();
}

#[tokio::main]
async fn main() {
    let state = match config!("event_store").unwrap().as_str() {
        "postgres" => postgres::application_state().await,
        _ => in_memory::application_state().await,
    };

    match config!("log_format").unwrap().as_str() {
        "json" => tracing_subscriber::fmt().json().init(),
        _ => tracing_subscriber::fmt::init(),
    }

    initialize(state.clone(), startup_commands_server_config(HOST.clone())).await;

    axum::Server::bind(&"0.0.0.0:3033".parse().unwrap())
        .serve(app(state).into_make_service())
        .await
        .unwrap();
}
