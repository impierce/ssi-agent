use agent_api_rest::app;
use agent_issuance::{startup_commands::startup_commands, state::initialize};
use agent_shared::config;
use agent_store::{in_memory, postgres};

#[tokio::main]
async fn main() {
    let state = match config!("event_store").unwrap().as_str() {
        "postgres" => postgres::application_state().await,
        _ => in_memory::application_state().await,
    };

    if let Ok(log_format) = config!("log_format") {
        if &log_format == "json" {
            tracing_subscriber::fmt().json().init()
        } else {
            tracing_subscriber::fmt::init()
        }
    } else {
        tracing_subscriber::fmt::init()
    }

    let url = config!("url").expect("AGENT_APPLICATION_URL is not set");

    tracing::info!("Application url: {:?}", url);

    let url = url::Url::parse(&url).unwrap();

    initialize(state.clone(), startup_commands(url)).await;

    let server = "0.0.0.0:3033".parse().unwrap();

    axum::Server::bind(&server)
        .serve(app(state).into_make_service())
        .await
        .unwrap();
}
