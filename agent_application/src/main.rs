#![allow(clippy::await_holding_lock)]

mod metadata;
mod probes;

use agent_api_rest::{app, ApplicationState};
use agent_event_publisher_http::EventPublisherHttp;
use agent_holder::services::HolderServices;
use agent_identity::services::IdentityServices;
use agent_issuance::{services::IssuanceServices, startup_commands::startup_commands};
use agent_secret_manager::{service::Service as _, subject::Subject};
use agent_shared::config::{config, LogFormat};
use agent_store::{in_memory, postgres, EventPublisher};
use agent_verification::services::VerificationServices;
use probes::liveness::healthz;
use std::sync::Arc;
use tokio::io;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> io::Result<()> {
    let tracing_subscriber = tracing_subscriber::registry()
        // Set the default logging level to `info`, equivalent to `RUST_LOG=info`
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()));

    match config().log_format {
        LogFormat::Json => tracing_subscriber.with(tracing_subscriber::fmt::layer().json()).init(),
        LogFormat::Text => tracing_subscriber.with(tracing_subscriber::fmt::layer()).init(),
    }

    let subject = Arc::new(Subject::new().await);

    let identity_services = Arc::new(IdentityServices::new(subject.clone()));
    let issuance_services = Arc::new(IssuanceServices::new(subject.clone()));
    let holder_services = Arc::new(HolderServices::new(subject.clone()));
    let verification_services = Arc::new(VerificationServices::new(subject.clone()));

    // TODO: Currently `issuance_event_publishers`, `holder_event_publishers` and `verification_event_publishers` are
    // exactly the same, which is weird. We need some sort of layer between `agent_application` and `agent_store` that
    // will provide a cleaner way of initializing the event publishers and sending them over to `agent_store`.
    let identity_event_publishers: Vec<Box<dyn EventPublisher>> = vec![Box::new(EventPublisherHttp::load().unwrap())];
    let issuance_event_publishers: Vec<Box<dyn EventPublisher>> = vec![Box::new(EventPublisherHttp::load().unwrap())];
    let holder_event_publishers: Vec<Box<dyn EventPublisher>> = vec![Box::new(EventPublisherHttp::load().unwrap())];
    let verification_event_publishers: Vec<Box<dyn EventPublisher>> =
        vec![Box::new(EventPublisherHttp::load().unwrap())];

    let (identity_state, issuance_state, holder_state, verification_state) =
        match agent_shared::config::config().event_store.type_ {
            agent_shared::config::EventStoreType::Postgres => (
                postgres::identity_state(identity_services, identity_event_publishers).await,
                postgres::issuance_state(issuance_services, issuance_event_publishers).await,
                postgres::holder_state(holder_services, holder_event_publishers).await,
                postgres::verification_state(verification_services, verification_event_publishers).await,
            ),
            agent_shared::config::EventStoreType::InMemory => (
                in_memory::identity_state(identity_services, identity_event_publishers).await,
                in_memory::issuance_state(issuance_services, issuance_event_publishers).await,
                in_memory::holder_state(holder_services, holder_event_publishers).await,
                in_memory::verification_state(verification_services, verification_event_publishers).await,
            ),
        };

    info!("{:?}", config());

    let url = &config().url.clone().expect("TODO: should never be None");

    info!("Application url: {}", url);

    agent_identity::state::initialize(&identity_state).await.unwrap();
    agent_issuance::state::initialize(&issuance_state, startup_commands(url.clone())).await;

    let app = app(ApplicationState {
        identity_state: Some(identity_state),
        issuance_state: Some(issuance_state),
        holder_state: Some(holder_state),
        verification_state: Some(verification_state),
    });

    let metadata_state = metadata::MetadataState {
        startup_instant: std::time::Instant::now(),
    };

    let metadata_router = axum::Router::new()
        .route("/version", axum::routing::get(metadata::version::version))
        .route("/info", axum::routing::get(metadata::info::info))
        // TODO: abbreviate to "/config"?
        .route("/v0/configuration", axum::routing::get(metadata::config::app_config))
        .with_state(metadata_state);
    let app = metadata_router.merge(app);

    let probes_router = axum::Router::new().route("/healthz", axum::routing::get(healthz));
    let app = probes_router.merge(app);

    let port = config().port.unwrap_or(3033);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    info!("HTTP API served at http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}
