#![allow(clippy::await_holding_lock)]

mod metadata;
mod probes;

use agent_api_http::{
    app,
    metrics::{metrics, track_metrics},
    sagas::{key_generation_saga::KeyGenerationSaga, key_removal_saga::KeyRemovalSaga},
    ApplicationState,
};
use agent_authorization::services::AuthorizationServices;
use agent_event_publisher_http::EventPublisherHttp;
use agent_event_publisher_nats::EventPublisherNats;
use agent_holder::services::HolderServices;
use agent_identity::services::{IdentityServices, ThisIsTheMainService};
use agent_issuance::{
    application::policies::issuer_metadata_synchronization_policy::IssuerMetadataSynchronizationPolicy,
    services::IssuanceServices,
};
use agent_secret_manager::{
    service::{SecretManagerServices, Service as _},
    subject::Subject,
};
use agent_shared::config::{config, EventStoreType};
use agent_store::{in_memory::InMemory, mongodb::MongoDB, postgres::Postgres, EventPublisher};
use agent_verification::services::VerificationServices;
use probes::liveness::healthz;
use std::sync::Arc;
use tokio::io;
use tower_http::cors::CorsLayer;
use tracing::info;

#[tokio::main]
async fn main() -> io::Result<()> {
    // TODO: Currently all these `*_event_publishers` are exactly the same, which is weird. We need some sort of layer
    // between `agent_application` and `agent_store` that will provide a cleaner way of initializing the event
    // publishers and sending them over to `agent_store`.
    let secret_manager_event_publishers: Vec<Box<dyn EventPublisher>> =
        vec![Box::new(EventPublisherHttp::load().unwrap())];
    let identity_event_publishers: Vec<Box<dyn EventPublisher>> = vec![Box::new(EventPublisherHttp::load().unwrap())];
    let issuance_event_publishers: Vec<Box<dyn EventPublisher>> = vec![
        Box::new(EventPublisherHttp::load().unwrap()),
        Box::new(EventPublisherNats::load().await.unwrap()),
    ];
    let library_event_publishers: Vec<Box<dyn EventPublisher>> = vec![Box::new(EventPublisherHttp::load().unwrap())];
    let authorization_event_publishers: Vec<Box<dyn EventPublisher>> =
        vec![Box::new(EventPublisherHttp::load().unwrap())];
    let holder_event_publishers: Vec<Box<dyn EventPublisher>> = vec![Box::new(EventPublisherHttp::load().unwrap())];
    let verification_event_publishers: Vec<Box<dyn EventPublisher>> =
        vec![Box::new(EventPublisherHttp::load().unwrap())];

    let subject = Arc::new(Subject::new().await);

    let secret_manager_services = Arc::new(SecretManagerServices::new().await);
    let secret_manager_state = match config().event_store.type_ {
        EventStoreType::Postgres => {
            let builder = Postgres::new().await;
            Arc::new(
                agent_store::secret_manager_state(
                    &builder,
                    secret_manager_services.clone(),
                    secret_manager_event_publishers,
                )
                .await,
            )
        }
        EventStoreType::MongoDb => {
            let builder = MongoDB::new().await;
            Arc::new(
                agent_store::secret_manager_state(
                    &builder,
                    secret_manager_services.clone(),
                    secret_manager_event_publishers,
                )
                .await,
            )
        }
        EventStoreType::InMemory => Arc::new(
            agent_store::secret_manager_state(
                &InMemory,
                secret_manager_services.clone(),
                secret_manager_event_publishers,
            )
            .await,
        ),
    };

    let identity_services = Arc::new(IdentityServices::new(subject.clone()));
    let identity_state = match config().event_store.type_ {
        EventStoreType::Postgres => {
            let builder = Postgres::new().await;
            Arc::new(agent_store::identity_state(&builder, identity_services.clone(), identity_event_publishers).await)
        }
        EventStoreType::MongoDb => {
            let builder = MongoDB::new().await;
            Arc::new(agent_store::identity_state(&builder, identity_services.clone(), identity_event_publishers).await)
        }
        EventStoreType::InMemory => {
            Arc::new(agent_store::identity_state(&InMemory, identity_services.clone(), identity_event_publishers).await)
        }
    };

    let this_is_the_main_service = Arc::new(
        ThisIsTheMainService::new(
            secret_manager_state.clone(),
            secret_manager_services.clone(),
            identity_state.clone(),
        )
        .await,
    );

    let authorization_services = Arc::new(AuthorizationServices::new(subject.clone()));
    let issuance_services = Arc::new(IssuanceServices::new(subject.clone(), this_is_the_main_service.clone()));
    let holder_services = Arc::new(HolderServices::new(subject.clone()));
    let verification_services = Arc::new(VerificationServices::new(subject.clone()));

    // TODO: Refactor this to reduce code duplication.
    let (library_state, authorization_state, issuance_state, holder_state, verification_state) = match config()
        .event_store
        .type_
    {
        EventStoreType::Postgres => {
            let builder = Postgres::new().await;

            let issuance_state =
                Arc::new(agent_store::issuance_state(&builder, issuance_services, issuance_event_publishers).await);

            let issuer_metadata_synchronization_policy =
                IssuerMetadataSynchronizationPolicy::new(issuance_state.clone());

            (
                Arc::new(
                    agent_store::library_state(
                        &builder,
                        library_event_publishers,
                        vec![Box::new(issuer_metadata_synchronization_policy)],
                    )
                    .await,
                ),
                Arc::new(
                    agent_store::authorization_state(&builder, authorization_services, authorization_event_publishers)
                        .await,
                ),
                issuance_state,
                Arc::new(agent_store::holder_state(&builder, holder_services, holder_event_publishers).await),
                Arc::new(
                    agent_store::verification_state(&builder, verification_services, verification_event_publishers)
                        .await,
                ),
            )
        }
        EventStoreType::MongoDb => {
            let builder = MongoDB::new().await;

            let issuance_state =
                Arc::new(agent_store::issuance_state(&builder, issuance_services, issuance_event_publishers).await);

            let issuer_metadata_synchronization_policy =
                IssuerMetadataSynchronizationPolicy::new(issuance_state.clone());

            (
                Arc::new(
                    agent_store::library_state(
                        &builder,
                        library_event_publishers,
                        vec![Box::new(issuer_metadata_synchronization_policy)],
                    )
                    .await,
                ),
                Arc::new(
                    agent_store::authorization_state(&builder, authorization_services, authorization_event_publishers)
                        .await,
                ),
                issuance_state,
                Arc::new(agent_store::holder_state(&builder, holder_services, holder_event_publishers).await),
                Arc::new(
                    agent_store::verification_state(&builder, verification_services, verification_event_publishers)
                        .await,
                ),
            )
        }
        EventStoreType::InMemory => {
            let issuance_state =
                Arc::new(agent_store::issuance_state(&InMemory, issuance_services, issuance_event_publishers).await);

            let issuer_metadata_synchronization_policy =
                IssuerMetadataSynchronizationPolicy::new(issuance_state.clone());

            (
                Arc::new(
                    agent_store::library_state(
                        &InMemory,
                        library_event_publishers,
                        vec![Box::new(issuer_metadata_synchronization_policy)],
                    )
                    .await,
                ),
                Arc::new(
                    agent_store::authorization_state(&InMemory, authorization_services, authorization_event_publishers)
                        .await,
                ),
                issuance_state,
                Arc::new(agent_store::holder_state(&InMemory, holder_services, holder_event_publishers).await),
                Arc::new(
                    agent_store::verification_state(&InMemory, verification_services, verification_event_publishers)
                        .await,
                ),
            )
        }
    };

    info!("{:?}", config());

    info!("Application url: {}", config().application_url);

    info!("Public url: {}", config().public_url);

    agent_authorization::state::initialize(&authorization_state)
        .await
        .unwrap();
    agent_identity::state::initialize(&identity_state).await.unwrap();
    agent_issuance::state::initialize(&issuance_state).await.unwrap();

    let key_generation_saga = KeyGenerationSaga::new(
        secret_manager_state.clone(),
        identity_state.clone(),
        identity_services.clone(),
    );
    let key_removal_saga = KeyRemovalSaga::new(
        secret_manager_state.clone(),
        identity_state.clone(),
        identity_services.clone(),
    );

    let app = app(ApplicationState {
        secret_manager_state: Some(secret_manager_state),
        identity_state: Some(identity_state),
        library_state: Some(library_state),
        authorization_state: Some(authorization_state),
        issuance_state: Some(issuance_state),
        holder_state: Some(holder_state),
        verification_state: Some(verification_state),

        key_generation_saga: Some(key_generation_saga),
        key_removal_saga: Some(key_removal_saga),
    });

    let metadata_state = metadata::MetadataState {
        startup_instant: std::time::Instant::now(),
    };

    // Add metadata routes
    let metadata_router = axum::Router::new()
        .route("/version", axum::routing::get(metadata::version::version))
        .route("/info", axum::routing::get(metadata::info::info))
        .route("/v0/configuration", axum::routing::get(metadata::config::configuration))
        .with_state(metadata_state);

    let app = metadata_router.merge(app);

    // Add probes routes
    let probes_router = axum::Router::new().route("/healthz", axum::routing::get(healthz));
    let mut app = probes_router.merge(app);

    if config().metrics.enabled {
        app = app.route_layer(axum::middleware::from_fn(track_metrics));
    }

    let port = config().application_url.port().unwrap_or(3033);

    let app_handle = tokio::spawn(start_server("HTTP API".to_string(), app, port));

    if config().metrics.enabled {
        let metrics_handle = tokio::spawn(start_server("Metrics".to_string(), metrics(), config().metrics.port));
        let _ = tokio::join!(app_handle, metrics_handle);
    } else {
        let _ = app_handle.await;
    }

    Ok(())
}

/// Start a server for a given `Router` on a given port.
async fn start_server(alias: String, router: axum::Router, port: u16) {
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await.unwrap();
    // Log the URL defined in the config for the HTTP API
    if alias == "HTTP API" {
        info!("HTTP API served at {}", config().application_url);
    } else {
        info!("{alias} served at {}", listener.local_addr().unwrap());
    }

    // CORS
    let router = if config().cors_enabled {
        info!("CORS (permissive) enabled for all routes");
        router.layer(CorsLayer::permissive())
    } else {
        router
    };

    axum::serve(listener, router).await.unwrap();
}
