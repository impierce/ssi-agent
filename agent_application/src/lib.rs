pub mod event_verification;
mod metadata;
pub mod openapi;
mod probes;
pub mod telemetry;

use agent_api_http::{app, metrics::track_metrics, ApiState, API_VERSION};
use agent_authorization::services::{AuthorizationServices, OAuth2AuthorizationRequestDomainServices};
use agent_event_publisher_http::HttpEventPublisher;
use agent_event_publisher_nats::NatsEventPublisher;
use agent_holder::{presentation::aggregate::Presentation, services::HolderServices};
use agent_identity::services::{IdentityServices, LinkedVerifiablePresentationSource};
use agent_issuance::{
    application::credential_configuration_projection::CredentialConfigurationProjection, services::IssuanceServices,
};
use agent_secret_manager::{service::Service as _, subject::Subject};
use agent_shared::config::{config, EventStoreType};
use agent_store::{in_memory::InMemory, mongodb::MongoDB, postgres::Postgres};
use agent_verification::services::VerificationServices;
pub use event_verification::{core_event_verifiers, EventVerificationError, EventVerificationReport, EventVerifier};
use probes::{
    liveness::{healthz, livez},
    readiness::{readyz, ReadinessState},
};
use shared_kernel::authorization::{ActorExtractor, NoActorExtractor};
use std::sync::Arc;
use tokio::io;
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

use agent_authorization::authorization_state;
use agent_holder::holder_state;
use agent_identity::identity_state;
use agent_issuance::issuance_state;
use agent_library::library_state;
use agent_verification::verification_state;
pub mod integration_projector;
pub use integration_projector::start_core_integration_projector;

pub struct ApplicationState {
    pub api: ApiState,
    pub domain_event_bus: shared_kernel::event_bus::EventBusHandle,
    pub integration_event_bus: shared_kernel::event_bus::EventBusHandle,
    pub event_verification: EventVerification,
    readiness: ReadinessState,
}

/// Publishes the holder's own signed presentations as Linked Verifiable Presentations.
struct HolderPresentations(Arc<HolderState>);

#[async_trait::async_trait]
impl LinkedVerifiablePresentationSource for HolderPresentations {
    async fn signed_presentation(
        &self,
        presentation_id: &str,
    ) -> anyhow::Result<Option<identity_credential::credential::Jwt>> {
        Ok(
            agent_shared::handlers::public_query_handler(presentation_id, &self.0.query.presentation)
                .await?
                .and_then(|Presentation { signed, .. }| signed),
        )
    }
}

impl ApplicationState {
    #[must_use]
    pub fn domain_event_bus(&self) -> shared_kernel::event_bus::EventBusHandle {
        self.domain_event_bus.clone()
    }

    #[must_use]
    pub fn integration_event_bus(&self) -> shared_kernel::event_bus::EventBusHandle {
        self.integration_event_bus.clone()
    }

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
}

pub async fn run() -> io::Result<()> {
    // Initialize the tracing subscriber before anything else so that all subsequent log output is captured.
    // Reading the log format triggers the configuration to be loaded first.
    let log_format = config().log_format.clone();
    let _telemetry_guard = telemetry::init_telemetry(&log_format);

    info!("Configuration loaded successfully");

    let subject = Arc::new(Subject::new().await);
    let state = state(subject).await?;
    state.verify_persisted_events().await;

    serve(router(state)).await
}

pub async fn state(subject: Arc<Subject>) -> io::Result<ApplicationState> {
    agent_shared::config::warn_deprecated_settings();
    let identity_services = |holder_state: &Arc<HolderState>| {
        Arc::new(IdentityServices {
            linked_verifiable_presentations: Arc::new(HolderPresentations(holder_state.clone())),
            ..IdentityServices::new(
                subject.clone(),
                config().public_url.clone(),
                config().iota_sponsoring_service_url.is_some(),
            )
        })
    };
    let authorization_services = Arc::new(AuthorizationServices::new(subject.clone()));
    let issuance_services = Arc::new(IssuanceServices::new(subject.clone()));
    let holder_services = Arc::new(HolderServices::new(subject.clone()));
    let verification_services = Arc::new(VerificationServices::new(subject.clone()));

    let domain_event_bus = shared_kernel::event_bus::EventBusHandle::default();
    let integration_event_bus = shared_kernel::event_bus::EventBusHandle::default();

    start_core_integration_projector(domain_event_bus.clone(), integration_event_bus.clone());

    if let Some(publisher) = HttpEventPublisher::from_config(&integration_event_bus) {
        publisher.spawn();
    }
    if let Some(publisher) = NatsEventPublisher::from_config(&integration_event_bus) {
        publisher.spawn();
    }

    let event_store_type = config().event_store.type_.clone();
    let event_verification;
    let readiness = ReadinessState::default();

    // TODO: Refactor this to reduce code duplication.
    let (identity_state, library_state, authorization_state, issuance_state, holder_state, verification_state) =
        match event_store_type {
            EventStoreType::Postgres => {
                let builder = Postgres::new().await;

                let issuance_state = Arc::new(issuance_state(&builder, issuance_services, &domain_event_bus).await);

                let (credential_configuration_projection, template_view_handle) =
                    CredentialConfigurationProjection::new(issuance_state.clone());

                let library_state = Arc::new(
                    library_state(
                        &builder,
                        &domain_event_bus,
                        vec![Box::new(credential_configuration_projection)],
                    )
                    .await,
                );
                assert!(
                    template_view_handle.set(library_state.query.template.clone()).is_ok(),
                    "template view already initialized"
                );

                let verification_state =
                    Arc::new(verification_state(&builder, verification_services, &domain_event_bus).await);

                let oauth2_authorization_request_domain_services = OAuth2AuthorizationRequestDomainServices::new(
                    Box::new(VerificationAuthorizationAdapter::new(verification_state.clone())),
                );

                let holder_state = Arc::new(holder_state(&builder, holder_services, &domain_event_bus).await);

                let states = (
                    Arc::new(identity_state(&builder, identity_services(&holder_state), &domain_event_bus).await),
                    library_state,
                    Arc::new(
                        authorization_state(
                            &builder,
                            authorization_services,
                            &domain_event_bus,
                            oauth2_authorization_request_domain_services,
                        )
                        .await,
                    ),
                    issuance_state,
                    holder_state,
                    verification_state,
                );
                event_verification = EventVerification::Postgres(builder);
                states
            }
            EventStoreType::MongoDb => {
                let builder = MongoDB::new().await;
                let mongo_source = agent_store::MongoEventSource::new(builder.client.clone());
                // 1. Stream live database writes from MongoDB Change Stream into the local EventBus.
                domain_event_bus.attach_source(mongo_source.clone());
                // 2. Register MongoDB as the persistent history reader for complete historical catch-up queries.
                domain_event_bus.set_history_reader(Arc::new(mongo_source));

                let issuance_state = Arc::new(issuance_state(&builder, issuance_services, &domain_event_bus).await);

                let (credential_configuration_projection, template_view_handle) =
                    CredentialConfigurationProjection::new(issuance_state.clone());

                let library_state = Arc::new(
                    library_state(
                        &builder,
                        &domain_event_bus,
                        vec![Box::new(credential_configuration_projection)],
                    )
                    .await,
                );
                assert!(
                    template_view_handle.set(library_state.query.template.clone()).is_ok(),
                    "template view already initialized"
                );

                let verification_state =
                    Arc::new(verification_state(&builder, verification_services, &domain_event_bus).await);

                let oauth2_authorization_request_domain_services = OAuth2AuthorizationRequestDomainServices::new(
                    Box::new(VerificationAuthorizationAdapter::new(verification_state.clone())),
                );

                let holder_state = Arc::new(holder_state(&builder, holder_services, &domain_event_bus).await);

                let states = (
                    Arc::new(identity_state(&builder, identity_services(&holder_state), &domain_event_bus).await),
                    library_state,
                    Arc::new(
                        authorization_state(
                            &builder,
                            authorization_services,
                            &domain_event_bus,
                            oauth2_authorization_request_domain_services,
                        )
                        .await,
                    ),
                    issuance_state,
                    holder_state,
                    verification_state,
                );
                event_verification = EventVerification::MongoDb(builder);
                states
            }
            EventStoreType::InMemory => {
                let issuance_state = Arc::new(issuance_state(&InMemory, issuance_services, &domain_event_bus).await);

                let (credential_configuration_projection, template_view_handle) =
                    CredentialConfigurationProjection::new(issuance_state.clone());

                let library_state = Arc::new(
                    library_state(
                        &InMemory,
                        &domain_event_bus,
                        vec![Box::new(credential_configuration_projection)],
                    )
                    .await,
                );
                assert!(
                    template_view_handle.set(library_state.query.template.clone()).is_ok(),
                    "template view already initialized"
                );

                let verification_state =
                    Arc::new(verification_state(&InMemory, verification_services, &domain_event_bus).await);

                let oauth2_authorization_request_domain_services = OAuth2AuthorizationRequestDomainServices::new(
                    Box::new(VerificationAuthorizationAdapter::new(verification_state.clone())),
                );

                let holder_state = Arc::new(holder_state(&InMemory, holder_services, &domain_event_bus).await);

                let states = (
                    Arc::new(identity_state(&InMemory, identity_services(&holder_state), &domain_event_bus).await),
                    library_state,
                    Arc::new(
                        authorization_state(
                            &InMemory,
                            authorization_services,
                            &domain_event_bus,
                            oauth2_authorization_request_domain_services,
                        )
                        .await,
                    ),
                    issuance_state,
                    holder_state,
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
    agent_identity::state::initialize(&identity_state)
        .await
        .map_err(io::Error::other)?;
    agent_identity::service::lifecycle::spawn_maintenance(&identity_state);
    agent_issuance::state::initialize(&issuance_state).await.unwrap();

    Ok(ApplicationState {
        api: ApiState {
            identity_state: Some(identity_state),
            library_state: Some(library_state),
            authorization_state: Some(authorization_state),
            issuance_state: Some(issuance_state),
            holder_state: Some(holder_state),
            verification_state: Some(verification_state),
            events_state: Some(Arc::new(agent_api_http::v0::events::EventsState::from(
                integration_event_bus.clone(),
            ))),
        },
        domain_event_bus,
        integration_event_bus,
        event_verification,
        readiness,
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
    let ApplicationState {
        api,
        domain_event_bus: _,
        integration_event_bus: _,
        event_verification: _,
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
        .route("/healthz", axum::routing::get(healthz))
        .route("/livez", axum::routing::get(livez))
        .route("/readyz", axum::routing::get(readyz))
        .with_state(readiness);
    let app = probes_router.merge(app);

    let app = openapi::router(config().serve_openapi_enabled).merge(app);

    // Record the OpenTelemetry HTTP request metrics (a no-op when OpenTelemetry is not enabled).
    app.route_layer(axum::middleware::from_fn(track_metrics))
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
