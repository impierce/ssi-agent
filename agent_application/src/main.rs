#![allow(clippy::await_holding_lock)]

mod metadata;
mod probes;

use agent_api_rest::{app, ApplicationState};
use agent_event_publisher_http::EventPublisherHttp;
use agent_holder::services::HolderServices;
use agent_identity::services::IdentityServices;
use agent_issuance::services::IssuanceServices;
use agent_secret_manager::{service::Service as _, subject::Subject};
use agent_shared::config::{config, EventStoreType};
use agent_store::{
    in_memory::{self, InMemory},
    mongodb::{self, MongoDB},
    postgres::{self, Postgres},
    EventPublisher,
};
use agent_verification::services::VerificationServices;
use probes::liveness::healthz;
use std::sync::Arc;
use tokio::io;
use tracing::info;

#[tokio::main]
async fn main() -> io::Result<()> {
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

    let (identity_state, issuance_state, holder_state, verification_state) = match config().event_store.type_ {
        EventStoreType::Postgres => {
            let builder = Postgres;
            (
                agent_store::identity_state::<Postgres>(&builder, identity_services, identity_event_publishers).await,
                postgres::issuance_state(issuance_services, issuance_event_publishers).await,
                agent_store::holder_state::<Postgres>(&builder, holder_services, holder_event_publishers).await,
                agent_store::verification_state::<Postgres>(
                    &builder,
                    verification_services,
                    verification_event_publishers,
                )
                .await,
            )
        }
        EventStoreType::MongoDb => {
            let builder = MongoDB::new().await;
            (
                agent_store::identity_state::<MongoDB>(&builder, identity_services, identity_event_publishers).await,
                mongodb::issuance_state(builder.client.clone(), issuance_services, issuance_event_publishers).await,
                agent_store::holder_state::<MongoDB>(&builder, holder_services, holder_event_publishers).await,
                agent_store::verification_state::<MongoDB>(
                    &builder,
                    verification_services,
                    verification_event_publishers,
                )
                .await,
            )
        }
        EventStoreType::InMemory => {
            let builder = InMemory;
            (
                agent_store::identity_state::<InMemory>(&builder, identity_services, identity_event_publishers).await,
                in_memory::issuance_state(issuance_services, issuance_event_publishers).await,
                agent_store::holder_state::<InMemory>(&builder, holder_services, holder_event_publishers).await,
                agent_store::verification_state::<InMemory>(
                    &builder,
                    verification_services,
                    verification_event_publishers,
                )
                .await,
            )
        }
    };

    info!("{:?}", config());

    info!("Application url: {}", config().application_url);

    info!("Public url: {}", config().public_url);

    agent_identity::state::initialize(&identity_state).await.unwrap();
    agent_issuance::state::initialize(&issuance_state).await.unwrap();

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
        .route("/v0/configuration", axum::routing::get(metadata::config::configuration))
        .with_state(metadata_state);
    let app = metadata_router.merge(app);

    let probes_router = axum::Router::new().route("/healthz", axum::routing::get(healthz));
    let app = probes_router.merge(app);

    let port = config().application_url.port().unwrap_or(3033);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    info!("HTTP API served at {}", config().application_url);
    axum::serve(listener, app).await?;

    Ok(())
}
