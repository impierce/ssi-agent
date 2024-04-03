use std::{str::FromStr, sync::Arc};

use agent_api_rest::app;
use agent_event_publisher_http::EventPublisherHttp;
use agent_issuance::{startup_commands::startup_commands, state::initialize};
use agent_secret_manager::secret_manager;
use agent_shared::config;
use agent_store::{in_memory, postgres, EventPublisher};
use agent_verification::services::VerificationServices;
use oid4vc_core::{client_metadata::ClientMetadata, DidMethod, SubjectSyntaxType};
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

    let verification_services = Arc::new(VerificationServices::new(
        Arc::new(secret_manager().await),
        // TODO: Temporary solution. Remove this once `ClientMetadata` is part of `RelyingPartyManager`.
        ClientMetadata::default()
            .with_subject_syntax_types_supported(vec![SubjectSyntaxType::Did(DidMethod::from_str("did:key").unwrap())]),
    ));

    let event_publishers: Vec<Box<dyn EventPublisher>> = vec![Box::new(EventPublisherHttp::load().unwrap())];

    let (issuance_state, verification_state) = match agent_shared::config!("event_store").unwrap().as_str() {
        "postgres" => (
            postgres::issuance_state().await,
            postgres::verification_state(verification_services, event_publishers).await,
        ),
        _ => (
            in_memory::issuance_state().await,
            in_memory::verification_state(verification_services, event_publishers).await,
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
