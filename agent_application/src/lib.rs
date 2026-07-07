mod metadata;
mod probes;

pub use agent_api_http::metrics::metrics;
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
use agent_store::{in_memory::InMemory, mongodb::MongoDB, postgres::Postgres, EventPublisher, EventValidationError};
use agent_verification::services::VerificationServices;
use probes::liveness::healthz;
use probes::readiness::{readyz, Readiness};
use shared_kernel::authorization::{ActorExtractor, NoActorExtractor};
use std::future::Future;
use std::pin::Pin;
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

/// A boxed, already-started future that, when awaited, performs the startup event-replay
/// validation pass (streaming + upcasting + deserializing every persisted event of every
/// aggregate) against whichever event store backend was selected in `state_with_validation`.
///
/// Boxed because the concrete backend (`Postgres`, `MongoDB`, or `InMemory`) that the future
/// closes over differs per `EventStoreType` match arm, and `state_with_validation` needs to return
/// a single, backend-agnostic type.
pub type ValidationFuture = Pin<Box<dyn Future<Output = Result<u64, EventValidationError>> + Send>>;

pub async fn run() -> io::Result<()> {
    let subject = Arc::new(Subject::new().await);
    let (state, validate_events) = state_with_validation(subject).await?;

    let readiness = Readiness::new();
    run_startup_validation(readiness.clone(), validate_events).await;

    serve(router_with_readiness(state, readiness)).await
}

/// Runs the startup event-replay validation pass (unless disabled via
/// `event_replay_validation = false`) and updates `readiness` with the outcome.
///
/// On failure this deliberately does *not* return an error or exit the process: the application
/// stays up and keeps serving `/healthz`, so an orchestrator observing a failing `/readyz` can
/// hold back traffic (or an old revision) instead of losing the deployment entirely.
async fn run_startup_validation(readiness: Readiness, validate_events: ValidationFuture) {
    if !config().event_replay_validation {
        info!(
            "Event replay validation is disabled (`event_replay_validation = false`); skipping the startup replay \
             validation pass."
        );
        readiness.set_ready();
        return;
    }

    info!("Validating persisted events for all aggregates (streaming + upcasting + deserializing) before serving traffic...");

    match validate_events.await {
        Ok(validated_count) => {
            info!("Event replay validation succeeded: {validated_count} event(s) validated across all aggregates.");
            readiness.set_ready();
        }
        Err(error) => {
            tracing::error!(
                "Event replay validation FAILED: {error}. The application will keep running and will keep serving \
                 `/healthz`, but `/readyz` will report 503 until this is resolved."
            );
            readiness.set_not_ready(error.to_string());
        }
    }
}

/// Builds the application state. This is a thin wrapper around [`state_with_validation`] for
/// callers that don't need the startup replay-validation future (e.g. tests).
pub async fn state(subject: Arc<Subject>) -> io::Result<ApplicationState> {
    let (state, _validate_events) = state_with_validation(subject).await?;
    Ok(state)
}

/// Builds the application state and, alongside it, a [`ValidationFuture`] that -- when awaited --
/// runs the startup event-replay validation pass against the same event store backend that was
/// just used to build `state`.
///
/// The backend builder (`Postgres`, `MongoDB`, or `InMemory`) is constructed and used locally
/// within each `EventStoreType` match arm and would otherwise be dropped at the end of the arm;
/// instead, it's moved into the returned future so `run()` can validate against it after `state()`
/// has already returned.
pub async fn state_with_validation(subject: Arc<Subject>) -> io::Result<(ApplicationState, ValidationFuture)> {
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

    // TODO: Refactor this to reduce code duplication.
    let (
        (identity_state, library_state, authorization_state, issuance_state, holder_state, verification_state),
        validate_events,
    ): (_, ValidationFuture) = match event_store_type {
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
                agent_store::verification_state(&builder, verification_services, verification_event_publishers).await,
            );

            let oauth2_authorization_request_domain_services = OAuth2AuthorizationRequestDomainServices::new(Box::new(
                VerificationAuthorizationAdapter::new(verification_state.clone()),
            ));

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

            let validate_events: ValidationFuture =
                Box::pin(async move { agent_store::validate_all_events(&builder).await });

            (states, validate_events)
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
                agent_store::verification_state(&builder, verification_services, verification_event_publishers).await,
            );

            let oauth2_authorization_request_domain_services = OAuth2AuthorizationRequestDomainServices::new(Box::new(
                VerificationAuthorizationAdapter::new(verification_state.clone()),
            ));

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

            let validate_events: ValidationFuture =
                Box::pin(async move { agent_store::validate_all_events(&builder).await });

            (states, validate_events)
        }
        EventStoreType::InMemory => {
            let issuance_state =
                Arc::new(agent_store::issuance_state(&InMemory, issuance_services, issuance_event_publishers).await);

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
                agent_store::verification_state(&InMemory, verification_services, verification_event_publishers).await,
            );

            let oauth2_authorization_request_domain_services = OAuth2AuthorizationRequestDomainServices::new(Box::new(
                VerificationAuthorizationAdapter::new(verification_state.clone()),
            ));

            let states = (
                Arc::new(agent_store::identity_state(&InMemory, identity_services, identity_event_publishers).await),
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

            // `InMemory` never persists events as JSON, so there is nothing to replay/validate;
            // `Postgres`/`MongoDB` return the actual validated count.
            let validate_events: ValidationFuture =
                Box::pin(async move { agent_store::validate_all_events(&InMemory).await });

            (states, validate_events)
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

    Ok((
        ApplicationState {
            identity_state: Some(identity_state),
            library_state: Some(library_state),
            authorization_state: Some(authorization_state),
            issuance_state: Some(issuance_state),
            holder_state: Some(holder_state),
            verification_state: Some(verification_state),
        },
        validate_events,
    ))
}

/// Builds the full core SSI agent Router (app + metadata + probes), with a fresh [`Readiness`]
/// that is already marked ready (i.e. `/readyz` behaves like `/healthz`). Use
/// [`router_with_readiness`] to wire up a `Readiness` handle that startup validation can flip.
pub fn router(application_state: ApplicationState) -> axum::Router {
    router_with_readiness(application_state, Readiness::new_ready())
}

/// Builds the full core SSI agent Router (app + metadata + probes), backing `/readyz` with the
/// given [`Readiness`] handle.
pub fn router_with_readiness(application_state: ApplicationState, readiness: Readiness) -> axum::Router {
    router_with_actor_extractor(application_state, NoActorExtractor, readiness)
        .merge(axum::Router::new().nest(API_VERSION, configuration_router()))
}

/// Builds the full core SSI agent Router with a custom actor extractor.
pub fn router_with_actor_extractor<E>(
    application_state: ApplicationState,
    actor_extractor: E,
    readiness: Readiness,
) -> axum::Router
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

    // Add probes routes. `/healthz` is an unconditional liveness probe; `/readyz` reflects
    // `readiness` (e.g. the outcome of the startup event-replay validation pass) and can report
    // `503` while the process otherwise keeps running.
    let probes_router = axum::Router::new()
        .route("/healthz", axum::routing::get(healthz))
        .route("/readyz", axum::routing::get(readyz))
        .with_state(readiness);
    let mut app = probes_router.merge(app);

    if config().metrics.enabled {
        app = app.route_layer(axum::middleware::from_fn(track_metrics));
    }

    app
}

/// Builds the application configuration router without the API version prefix.
pub fn configuration_router() -> axum::Router {
    axum::Router::new().route("/configuration", axum::routing::get(metadata::config::configuration))
}

async fn serve(app: axum::Router) -> io::Result<()> {
    let port = config().application_url.port().unwrap_or(3033);

    let app_handle = tokio::spawn(start_server("HTTP API".to_string(), app, port));

    let servers = async {
        if config().metrics.enabled {
            let metrics_handle = tokio::spawn(start_server("Metrics".to_string(), metrics(), config().metrics.port));
            let _ = tokio::join!(app_handle, metrics_handle);
        } else {
            let _ = app_handle.await;
        }
    };

    tokio::select! {
        _ = servers => {},
        _ = shutdown_signal() => {
            info!("Shutdown signal received, exiting immediately.");
        }
    }

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

/// Resolves when the process receives a termination signal: `SIGTERM` (sent by Kubernetes /
/// `docker stop`) or `SIGINT` (Ctrl-C). Because the agent runs as PID 1 in the container, the
/// kernel applies no default signal disposition, so we must handle these explicitly or the
/// process would ignore `SIGTERM` and only die once the orchestrator escalates to `SIGKILL`.
async fn shutdown_signal() {
    use tokio::signal;

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = signal::ctrl_c() => {},
        _ = terminate => {},
    }
}

/// Boot-level tests for the startup event-replay validation: the outcome of the
/// [`ValidationFuture`] awaited by [`run_startup_validation`] must be reflected by `/readyz`,
/// exactly as it is when `run()` wires the two together.
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use cqrs_es::persist::PersistenceError;
    use tower::ServiceExt;

    /// A minimal router exposing only the probe that `run()` backs with the same [`Readiness`]
    /// handle it passes to [`run_startup_validation`].
    fn readyz_router(readiness: Readiness) -> axum::Router {
        axum::Router::new()
            .route("/readyz", axum::routing::get(readyz))
            .with_state(readiness)
    }

    async fn get_readyz(readiness: Readiness) -> (StatusCode, serde_json::Value) {
        let response = readyz_router(readiness)
            .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn successful_startup_validation_is_reflected_by_readyz() {
        let readiness = Readiness::new();

        run_startup_validation(readiness.clone(), Box::pin(async { Ok(42) })).await;

        let (status, body) = get_readyz(readiness).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ready");
    }

    #[tokio::test]
    async fn failed_startup_validation_keeps_readyz_at_503_with_the_descriptive_reason() {
        let readiness = Readiness::new();
        let error = EventValidationError {
            aggregate_type: "nonce",
            validated_count: 7,
            source: PersistenceError::DeserializationError("missing field `is_redeemed`".into()),
        };

        run_startup_validation(readiness.clone(), Box::pin(async move { Err(error) })).await;

        let (status, body) = get_readyz(readiness).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "not_ready");
        assert_eq!(
            body["reason"],
            "event replay validation failed for aggregate type `nonce` after successfully validating 7 event(s): \
             missing field `is_redeemed`"
        );
    }
}
