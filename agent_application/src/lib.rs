pub mod credential_metrics;
mod metadata;
mod probes;
pub mod telemetry;

use agent_api_http::{app, metrics::track_metrics, ApplicationState, API_VERSION};
use agent_authorization::services::{AuthorizationServices, OAuth2AuthorizationRequestDomainServices};
use agent_event_publisher_http::EventPublisherHttp;
use agent_event_publisher_nats::EventPublisherNats;
use agent_holder::services::HolderServices;
use agent_identity::services::IdentityServices;
use agent_issuance::{
    application::credential_configuration_projection::CredentialConfigurationProjection, services::IssuanceServices,
};
use agent_secret_manager::{service::Service as _, subject::Subject};
use agent_shared::config::{config, EventStoreType};
use agent_store::{in_memory::InMemory, mongodb::MongoDB, postgres::Postgres, EventPublisher};
use agent_verification::services::VerificationServices;
use probes::liveness::healthz;
use shared_kernel::authorization::{ActorExtractor, NoActorExtractor};
use std::sync::Arc;
use tokio::io;
use tower_http::cors::CorsLayer;
use tracing::info;
use verification_authorization::VerificationAuthorizationAdapter;

// Re-export states
pub use agent_authorization::state::AuthorizationState;
pub use agent_holder::state::HolderState;
pub use agent_identity::state::IdentityState;
pub use agent_issuance::state::{IssuanceState, SERVER_CONFIG_ID};
pub use agent_library::state::LibraryState;
pub use agent_verification::state::VerificationState;

pub async fn run() -> io::Result<()> {
    // Initialize the tracing subscriber before anything else so that all subsequent log output is captured. Reading
    // the log format triggers the configuration to be loaded first.
    let log_format = config().log_format.clone();
    let _telemetry_guard = telemetry::init_telemetry(&log_format);

    info!("Configuration loaded successfully");

    let subject = Arc::new(Subject::new().await);
    let state = state(subject).await?;

    serve(router(state)).await
}

pub async fn state(subject: Arc<Subject>) -> io::Result<ApplicationState> {
    let identity_services = Arc::new(IdentityServices::new(subject.clone()));
    let authorization_services = Arc::new(AuthorizationServices::new(subject.clone()));
    let issuance_services = Arc::new(IssuanceServices::new(subject.clone()));
    let holder_services = Arc::new(HolderServices::new(subject.clone()));
    let verification_services = Arc::new(VerificationServices::new(subject.clone()));

    // TODO: Currently all these `*_event_publishers` are exactly the same, which is weird. We need some sort of layer
    // between `agent_application` and `agent_store` that will provide a cleaner way of initializing the event
    // publishers and sending them over to `agent_store`.
    let identity_event_publishers: Vec<Box<dyn EventPublisher>> = EventPublisherHttp::load()
        .unwrap()
        .into_iter()
        .map(|p| Box::new(p) as Box<dyn EventPublisher>)
        .collect();
    // Issuance events are also published to NATS.
    let mut issuance_event_publishers: Vec<Box<dyn EventPublisher>> = EventPublisherHttp::load()
        .unwrap()
        .into_iter()
        .map(|p| Box::new(p) as Box<dyn EventPublisher>)
        .collect();
    issuance_event_publishers.push(Box::new(EventPublisherNats::load().await.unwrap()));
    let library_event_publishers: Vec<Box<dyn EventPublisher>> = EventPublisherHttp::load()
        .unwrap()
        .into_iter()
        .map(|p| Box::new(p) as Box<dyn EventPublisher>)
        .collect();
    let authorization_event_publishers: Vec<Box<dyn EventPublisher>> = EventPublisherHttp::load()
        .unwrap()
        .into_iter()
        .map(|p| Box::new(p) as Box<dyn EventPublisher>)
        .collect();
    let holder_event_publishers: Vec<Box<dyn EventPublisher>> = EventPublisherHttp::load()
        .unwrap()
        .into_iter()
        .map(|p| Box::new(p) as Box<dyn EventPublisher>)
        .collect();
    let verification_event_publishers: Vec<Box<dyn EventPublisher>> = EventPublisherHttp::load()
        .unwrap()
        .into_iter()
        .map(|p| Box::new(p) as Box<dyn EventPublisher>)
        .collect();

    let event_store_type = config().event_store.type_.clone();

    // Counts credentials (excluding deleted ones) based on the `Credential` events and exposes the count as a
    // gauge to both the Prometheus `/metrics` endpoint and the OpenTelemetry meter provider.
    let credential_count_projection = credential_metrics::CredentialCountProjection::default();

    // TODO: Refactor this to reduce code duplication.
    let (identity_state, library_state, authorization_state, issuance_state, holder_state, verification_state) =
        match event_store_type {
            EventStoreType::Postgres => {
                let builder = Postgres::new().await;

                let issuance_state = Arc::new(
                    agent_store::issuance_state_with_credential_queries(
                        &builder,
                        issuance_services,
                        issuance_event_publishers,
                        vec![Box::new(credential_count_projection.clone())],
                    )
                    .await,
                );

                let (credential_configuration_projection, template_view_handle) =
                    CredentialConfigurationProjection::new(issuance_state.clone());

                let library_state = Arc::new(
                    agent_store::library_state(
                        &builder,
                        library_event_publishers,
                        vec![Box::new(credential_configuration_projection)],
                    )
                    .await,
                );
                assert!(
                    template_view_handle.set(library_state.query.template.clone()).is_ok(),
                    "template view already initialized"
                );

                let verification_state = Arc::new(
                    agent_store::verification_state(&builder, verification_services, verification_event_publishers)
                        .await,
                );

                let oauth2_authorization_request_domain_services = OAuth2AuthorizationRequestDomainServices::new(
                    Box::new(VerificationAuthorizationAdapter::new(verification_state.clone())),
                );

                (
                    Arc::new(agent_store::identity_state(&builder, identity_services, identity_event_publishers).await),
                    library_state,
                    Arc::new(
                        agent_store::authorization_state(
                            &builder,
                            authorization_services,
                            authorization_event_publishers,
                            oauth2_authorization_request_domain_services,
                        )
                        .await,
                    ),
                    issuance_state,
                    Arc::new(agent_store::holder_state(&builder, holder_services, holder_event_publishers).await),
                    verification_state,
                )
            }
            EventStoreType::MongoDb => {
                let builder = MongoDB::new().await;

                let issuance_state = Arc::new(
                    agent_store::issuance_state_with_credential_queries(
                        &builder,
                        issuance_services,
                        issuance_event_publishers,
                        vec![Box::new(credential_count_projection.clone())],
                    )
                    .await,
                );

                let (credential_configuration_projection, template_view_handle) =
                    CredentialConfigurationProjection::new(issuance_state.clone());

                let library_state = Arc::new(
                    agent_store::library_state(
                        &builder,
                        library_event_publishers,
                        vec![Box::new(credential_configuration_projection)],
                    )
                    .await,
                );
                assert!(
                    template_view_handle.set(library_state.query.template.clone()).is_ok(),
                    "template view already initialized"
                );

                let verification_state = Arc::new(
                    agent_store::verification_state(&builder, verification_services, verification_event_publishers)
                        .await,
                );

                let oauth2_authorization_request_domain_services = OAuth2AuthorizationRequestDomainServices::new(
                    Box::new(VerificationAuthorizationAdapter::new(verification_state.clone())),
                );

                (
                    Arc::new(agent_store::identity_state(&builder, identity_services, identity_event_publishers).await),
                    library_state,
                    Arc::new(
                        agent_store::authorization_state(
                            &builder,
                            authorization_services,
                            authorization_event_publishers,
                            oauth2_authorization_request_domain_services,
                        )
                        .await,
                    ),
                    issuance_state,
                    Arc::new(agent_store::holder_state(&builder, holder_services, holder_event_publishers).await),
                    verification_state,
                )
            }
            EventStoreType::InMemory => {
                let issuance_state = Arc::new(
                    agent_store::issuance_state_with_credential_queries(
                        &InMemory,
                        issuance_services,
                        issuance_event_publishers,
                        vec![Box::new(credential_count_projection.clone())],
                    )
                    .await,
                );

                let (credential_configuration_projection, template_view_handle) =
                    CredentialConfigurationProjection::new(issuance_state.clone());

                let library_state = Arc::new(
                    agent_store::library_state(
                        &InMemory,
                        library_event_publishers,
                        vec![Box::new(credential_configuration_projection)],
                    )
                    .await,
                );
                assert!(
                    template_view_handle.set(library_state.query.template.clone()).is_ok(),
                    "template view already initialized"
                );

                let verification_state = Arc::new(
                    agent_store::verification_state(&InMemory, verification_services, verification_event_publishers)
                        .await,
                );

                let oauth2_authorization_request_domain_services = OAuth2AuthorizationRequestDomainServices::new(
                    Box::new(VerificationAuthorizationAdapter::new(verification_state.clone())),
                );

                (
                    Arc::new(
                        agent_store::identity_state(&InMemory, identity_services, identity_event_publishers).await,
                    ),
                    library_state,
                    Arc::new(
                        agent_store::authorization_state(
                            &InMemory,
                            authorization_services,
                            authorization_event_publishers,
                            oauth2_authorization_request_domain_services,
                        )
                        .await,
                    ),
                    issuance_state,
                    Arc::new(agent_store::holder_state(&InMemory, holder_services, holder_event_publishers).await),
                    verification_state,
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

    // Seed the credential count metric from the persisted credentials before new events arrive.
    credential_count_projection
        .seed(&issuance_state.query.all_credentials)
        .await;

    Ok(ApplicationState {
        identity_state: Some(identity_state),
        library_state: Some(library_state),
        authorization_state: Some(authorization_state),
        issuance_state: Some(issuance_state),
        holder_state: Some(holder_state),
        verification_state: Some(verification_state),
    })
}

/// Builds the full core SSI agent Router (app + metadata + probes).
pub fn router(application_state: ApplicationState) -> axum::Router {
    router_with_actor_extractor(application_state, NoActorExtractor)
        .merge(axum::Router::new().nest(API_VERSION, configuration_router()))
}

/// Builds the full core SSI agent Router with a custom actor extractor.
pub fn router_with_actor_extractor<E>(application_state: ApplicationState, actor_extractor: E) -> axum::Router
where
    E: ActorExtractor,
{
    let actor_extractor = Arc::new(actor_extractor);
    let app = app(application_state, actor_extractor);

    let metadata_state = metadata::MetadataState {
        startup_instant: std::time::Instant::now(),
    };

    // Add metadata routes
    let metadata_router = axum::Router::new()
        .route("/version", axum::routing::get(metadata::version::version))
        .route("/info", axum::routing::get(metadata::info::info))
        .with_state(metadata_state);

    let app = metadata_router.merge(app);

    // Add probes routes
    let probes_router = axum::Router::new().route("/healthz", axum::routing::get(healthz));
    let app = probes_router.merge(app);

    // Record the OpenTelemetry HTTP request metrics (a no-op when OpenTelemetry is not enabled).
    app.route_layer(axum::middleware::from_fn(track_metrics))
}

/// Builds the application configuration router without the API version prefix.
pub fn configuration_router() -> axum::Router {
    axum::Router::new().route("/configuration", axum::routing::get(metadata::config::configuration))
}

async fn serve(app: axum::Router) -> io::Result<()> {
    let port = config().application_url.port().unwrap_or(3033);

    start_server("HTTP API".to_string(), app, port).await;

    Ok(())
}

/// Start a server for a given `Router` on a given port.
pub async fn start_server(alias: String, router: axum::Router, port: u16) {
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
