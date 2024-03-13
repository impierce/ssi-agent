use agent_api_rest::app;
use agent_issuance::{startup_commands::startup_commands, state::initialize};
use agent_shared::config;
use agent_store::{in_memory, postgres};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    let tracing_subscriber = tracing_subscriber::registry()
        // Set the default logging level to `info`, equivalent to `RUST_LOG=info`
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()));

    match config!("log_format") {
        Ok(log_format) if log_format == "json" => {
            tracing_subscriber.with(tracing_subscriber::fmt::layer().json()).init()
        }
        _ => tracing_subscriber.with(tracing_subscriber::fmt::layer()).init(),
    }

    let state = match config!("event_store").unwrap().as_str() {
        "postgres" => postgres::application_state().await,
        _ => in_memory::application_state().await,
    };

    let url = config!("url").expect("AGENT_APPLICATION_URL is not set");

    info!("Application url: {:?}", url);

    let url = url::Url::parse(&url).unwrap();

    initialize(state.clone(), startup_commands(url)).await;

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3033").await.unwrap();
    info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app(state)).await.unwrap();
}
