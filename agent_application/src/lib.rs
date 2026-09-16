mod metadata;
mod probes;
pub mod telemetry;

use agent_api_http::{app, metrics::track_metrics, ApiState, API_VERSION};
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
pub use agent_store::event_verification::{core_event_verifiers, EventVerifier};
use agent_store::{
    event_verification::{EventVerificationError, EventVerificationReport},
    in_memory::InMemory,
    mongodb::MongoDB,
    postgres::Postgres,
    EventPublisher,
};
use agent_verification::services::VerificationServices;
use probes::{
    liveness::healthz,
    readiness::{readyz, ReadinessState},
};
use shared_kernel::authorization::{ActorExtractor, NoActorExtractor};
use std::sync::Arc;
use tokio::{io, sync::oneshot};
use tower_http::cors::CorsLayer;
use tracing::{error, info};
use verification_authorization::VerificationAuthorizationAdapter;

// Re-export states
pub use agent_authorization::state::AuthorizationState;
pub use agent_holder::state::HolderState;
pub use agent_identity::state::IdentityState;
pub use agent_issuance::state::{IssuanceState, SERVER_CONFIG_ID};
pub use agent_library::state::LibraryState;
pub use agent_verification::state::VerificationState;

pub struct ApplicationState {
    pub api: ApiState,
    pub event_verification: Arc<EventVerification>,
    readiness: ReadinessState,
}

impl ApplicationState {
    pub async fn verify_persisted_events(&self) {
        verify_persisted_events(self.event_verification.verify_events().await, &self.readiness);
    }

    pub async fn verify_persisted_events_with(&self, verifiers: &[EventVerifier]) {
        verify_persisted_events(
            self.event_verification.verify_events_with(verifiers).await,
            &self.readiness,
        );
    }

    pub fn mark_ready(&self) {
        self.readiness.mark_ready();
    }

    pub fn mark_not_ready(&self) {
        self.readiness.mark_not_ready();
    }
}

pub enum EventVerification {
    Postgres(Postgres),
    MongoDb(MongoDB),
    InMemory,
}

impl EventVerification {
    pub async fn verify_events(&self) -> Result<EventVerificationReport, EventVerificationError> {
        self.verify_events_with(core_event_verifiers()).await
    }

    pub async fn verify_events_with(
        &self,
        verifiers: &[EventVerifier],
    ) -> Result<EventVerificationReport, EventVerificationError> {
        match self {
            Self::Postgres(store) => store.verify_events_with(verifiers).await,
            Self::MongoDb(store) => store.verify_events_with(verifiers).await,
            Self::InMemory => Ok(EventVerificationReport::default()),
        }
    }

    async fn writer_lease_lost(&self) {
        match self {
            Self::MongoDb(store) => store.writer_lease_lost().await,
            Self::Postgres(store) => store.writer_lease_lost().await,
            Self::InMemory => std::future::pending().await,
        }
    }

    async fn shutdown(&self) {
        match self {
            Self::MongoDb(store) => store.shutdown().await,
            Self::Postgres(store) => store.shutdown().await,
            Self::InMemory => {}
        }
    }
}

pub async fn run() -> io::Result<()> {
    // Initialize the tracing subscriber before anything else so that all subsequent log output is captured.
    // Reading the log format triggers the configuration to be loaded first.
    let log_format = config().log_format.clone();
    let _telemetry_guard = telemetry::init_telemetry(&log_format);

    info!("Configuration loaded successfully");

    let port = config().application_url.port().unwrap_or(3033);
    let std_listener = std::net::TcpListener::bind(format!("0.0.0.0:{port}"))?;
    std_listener.set_nonblocking(true)?;
    let bootstrap_listener = tokio::net::TcpListener::from_std(std_listener.try_clone()?)?;
    let main_listener = tokio::net::TcpListener::from_std(std_listener)?;

    let readiness = ReadinessState::default();
    let bootstrap_readiness = readiness.clone();
    let (bootstrap_shutdown, bootstrap_shutdown_received) = oneshot::channel();
    let bootstrap_server = tokio::spawn(async move {
        axum::serve(bootstrap_listener, bootstrap_router(bootstrap_readiness))
            .with_graceful_shutdown(async {
                let _ = bootstrap_shutdown_received.await;
            })
            .await
    });

    let subject = Arc::new(Subject::new().await);
    let state_result = state_with_readiness(subject, readiness.clone(), &[]).await;
    let _ = bootstrap_shutdown.send(());
    bootstrap_server.await.map_err(io::Error::other)??;
    let state = state_result?;

    let runtime = state.event_verification.clone();
    let shutdown_runtime = runtime.clone();
    let shutdown_readiness = readiness.clone();
    let shutdown = async move {
        tokio::select! {
            () = sigterm() => info!("SIGTERM received; starting graceful shutdown"),
            () = shutdown_runtime.writer_lease_lost() => error!("Event-store writer lease lost; terminating process"),
        }
        shutdown_readiness.mark_not_ready();
    };

    info!("HTTP API served at {}", config().application_url);
    let server_result = axum::serve(main_listener, with_cors(router(state)))
        .with_graceful_shutdown(shutdown)
        .await;
    readiness.mark_not_ready();
    runtime.shutdown().await;

    server_result
}

pub async fn state(subject: Arc<Subject>) -> io::Result<ApplicationState> {
    state_with_readiness(subject, ReadinessState::default(), &[]).await
}

/// Builds application state, declaring aggregate types whose projections are owned and persisted by
/// a downstream crate rather than rebuilt from events by the core.
///
/// Events of a declared type are skipped during replay. Undeclared types with no registered replay
/// job abort startup — the core must never rebuild a projection it has silently lost.
pub async fn state_with_external_aggregates(
    subject: Arc<Subject>,
    external_aggregates: &[&str],
) -> io::Result<ApplicationState> {
    state_with_readiness(subject, ReadinessState::default(), external_aggregates).await
}

async fn state_with_readiness(
    subject: Arc<Subject>,
    readiness: ReadinessState,
    external_aggregates: &[&str],
) -> io::Result<ApplicationState> {
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
    let event_verification;

    // TODO: Refactor this to reduce code duplication.
    let (identity_state, library_state, authorization_state, issuance_state, holder_state, verification_state) =
        match event_store_type {
            EventStoreType::Postgres => {
                let builder = Postgres::new().await;

                let issuance_state =
                    Arc::new(agent_store::issuance_state(&builder, issuance_services, issuance_event_publishers).await);

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

                let states = (
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
                );
                builder.acquire_writer_lease().await.map_err(io::Error::other)?;

                let reports = match builder.replay_views(external_aggregates).await {
                    Ok(reports) => reports,
                    Err(replay_error) => {
                        error!(error = %replay_error, "Failed to replay in-memory projections; terminating startup");
                        builder.shutdown().await;
                        return Err(io::Error::other(replay_error));
                    }
                };
                log_replay_reports(reports);
                event_verification = EventVerification::Postgres(builder);
                states
            }
            EventStoreType::MongoDb => {
                let builder = MongoDB::new().await;

                let issuance_state =
                    Arc::new(agent_store::issuance_state(&builder, issuance_services, issuance_event_publishers).await);

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

                let states = (
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
                );
                builder.acquire_writer_lease().await.map_err(io::Error::other)?;

                let reports = match builder.replay_views(external_aggregates).await {
                    Ok(reports) => reports,
                    Err(replay_error) => {
                        error!(error = %replay_error, "Failed to replay in-memory projections; terminating startup");
                        builder.shutdown().await;
                        return Err(io::Error::other(replay_error));
                    }
                };
                log_replay_reports(reports);
                event_verification = EventVerification::MongoDb(builder);
                states
            }
            EventStoreType::InMemory => {
                let issuance_state = Arc::new(
                    agent_store::issuance_state(&InMemory, issuance_services, issuance_event_publishers).await,
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

                let states = (
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
                );
                event_verification = EventVerification::InMemory;
                states
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

    let application_state = ApplicationState {
        api: ApiState {
            identity_state: Some(identity_state),
            library_state: Some(library_state),
            authorization_state: Some(authorization_state),
            issuance_state: Some(issuance_state),
            holder_state: Some(holder_state),
            verification_state: Some(verification_state),
        },
        event_verification: Arc::new(event_verification),
        readiness,
    };
    if matches!(
        application_state.event_verification.as_ref(),
        EventVerification::MongoDb(_) | EventVerification::Postgres(_)
    ) {
        application_state.mark_ready();
    } else {
        application_state.verify_persisted_events().await;
    }

    Ok(application_state)
}

fn log_replay_reports(summary: agent_store::replay::ReplaySummary) {
    for (aggregate_type, events_skipped) in summary.skipped {
        info!(
            aggregate_type = %aggregate_type,
            events_skipped,
            "Skipped replay for an externally owned aggregate type; the downstream crate persists this projection itself"
        );
    }

    for report in summary.reports {
        info!(
            aggregate_type = report.aggregate_type,
            events_replayed = report.events_replayed,
            aggregates_replayed = report.aggregates_replayed,
            duration_ms = report.duration.as_millis(),
            "Replayed in-memory projections"
        );
    }
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
    let ApplicationState {
        api,
        event_verification,
        readiness,
    } = application_state;
    let actor_extractor = Arc::new(actor_extractor);
    let app = app(api, actor_extractor);

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
    let probes_router = axum::Router::new()
        .route("/livez", axum::routing::get(healthz))
        .route("/healthz", axum::routing::get(healthz))
        .route("/readyz", axum::routing::get(readyz))
        .with_state(readiness);
    let app = probes_router.merge(app);

    // Record the OpenTelemetry HTTP request metrics (a no-op when OpenTelemetry is not enabled).
    app.route_layer(axum::middleware::from_fn(track_metrics))
        .layer(axum::Extension(event_verification))
}

fn bootstrap_router(readiness: ReadinessState) -> axum::Router {
    with_cors(
        axum::Router::new()
            .route("/livez", axum::routing::get(healthz))
            .route("/healthz", axum::routing::get(healthz))
            .route("/readyz", axum::routing::get(readyz))
            .fallback(|| async { axum::http::StatusCode::SERVICE_UNAVAILABLE })
            .with_state(readiness),
    )
}

/// Builds the application configuration router without the API version prefix.
pub fn configuration_router() -> axum::Router {
    axum::Router::new().route(
        "/configuration",
        axum::routing::get(agent_api_http::v0::configuration::configuration),
    )
}

fn verify_persisted_events(
    result: Result<EventVerificationReport, EventVerificationError>,
    readiness: &ReadinessState,
) {
    match result {
        Ok(report) if report.is_compatible() => {
            info!(checked = report.checked, "Persisted event verification succeeded");
            readiness.mark_ready();
        }
        Ok(report) => {
            for event in report.incompatible {
                error!(
                    aggregate_type = %event.aggregate_type,
                    aggregate_id = %event.aggregate_id,
                    sequence = event.sequence,
                    event_type = %event.event_type,
                    event_version = %event.event_version,
                    reason = %event.reason,
                    "Incompatible persisted event"
                );
            }
        }
        Err(error) => {
            error!(%error, "Failed to verify persisted events");
        }
    }
}

#[cfg(unix)]
async fn sigterm() {
    use tokio::signal::unix::{signal, SignalKind};

    let mut signal = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
    signal.recv().await;
}

#[cfg(not(unix))]
async fn sigterm() {
    std::future::pending().await
}

fn with_cors(router: axum::Router) -> axum::Router {
    if config().cors_enabled {
        info!("CORS (permissive) enabled for all routes");
        router.layer(CorsLayer::permissive())
    } else {
        router
    }
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
    let router = with_cors(router);

    axum::serve(listener, router).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt as _;

    async fn status(router: &axum::Router, path: &str) -> axum::http::StatusCode {
        router
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn bootstrap_router_serves_probes_and_blocks_application_routes() {
        let readiness = ReadinessState::default();
        let router = bootstrap_router(readiness.clone());

        assert_eq!(status(&router, "/livez").await, axum::http::StatusCode::OK);
        assert_eq!(status(&router, "/healthz").await, axum::http::StatusCode::OK);
        assert_eq!(
            status(&router, "/readyz").await,
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            status(&router, "/v0/documents").await,
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );

        readiness.mark_ready();
        assert_eq!(status(&router, "/readyz").await, axum::http::StatusCode::OK);
    }
}
