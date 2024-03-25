use std::sync::Arc;

use agent_api_rest::app;
use agent_event_publisher_http::EventPublisherHttp;
use agent_issuance::{startup_commands::startup_commands, state::initialize};
use agent_shared::{config, secret_manager::secret_manager};
use agent_store::{in_memory, postgres, OutboundAdapter};
use agent_verification::services::VerificationServices;
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

    let verification_services = Arc::new(VerificationServices::new(Arc::new(secret_manager().await)));

    let outbound_adapters: Vec<Box<dyn OutboundAdapter>> = vec![Box::new(EventPublisherHttp::load().unwrap())];

    let (issuance_state, verification_state) = match agent_shared::config!("event_store").unwrap().as_str() {
        "postgres" => (
            postgres::issuance_state().await,
            postgres::verification_state(verification_services, outbound_adapters).await,
        ),
        _ => (
            in_memory::issuance_state().await,
            in_memory::verification_state(verification_services, outbound_adapters).await,
        ),
    };

    let url = config!("url").expect("AGENT_APPLICATION_URL is not set");
    // TODO: Temporary solution. In the future we need to read these kinds of values from a config file.
    std::env::set_var("AGENT_VERIFICATION_URL", &url);

    info!("Application url: {:?}", url);

    let url = url::Url::parse(&url).unwrap();

    initialize(&issuance_state, startup_commands(url)).await;

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3033").await.unwrap();
    info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app((issuance_state, verification_state)))
        .await
        .unwrap();
}
