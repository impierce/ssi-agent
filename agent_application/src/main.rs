use agent_api_rest::app;
use agent_event_publisher_http::EventPublisherHttp;
use agent_issuance::{startup_commands::startup_commands, state::initialize};
use agent_secret_manager::{secret_manager, subject::Subject};
use agent_shared::{
    config,
    domain_linkage::create_did_configuration_resource,
    metadata::{load_metadata, Metadata},
};
use agent_store::{in_memory, postgres, EventPublisher};
use agent_verification::services::VerificationServices;
use axum::{http::StatusCode, routing::get, Json};
use identity_document::service::{Service, ServiceEndpoint};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    let metadata: Metadata = load_metadata();

    let tracing_subscriber = tracing_subscriber::registry()
        // Set the default logging level to `info`, equivalent to `RUST_LOG=info`
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()));

    match config!("log_format", String) {
        Ok(log_format) if log_format == "json" => {
            tracing_subscriber.with(tracing_subscriber::fmt::layer().json()).init()
        }
        _ => tracing_subscriber.with(tracing_subscriber::fmt::layer()).init(),
    }

    let verification_services = Arc::new(VerificationServices::new(
        Arc::new(Subject {
            secret_manager: secret_manager().await,
        }),
        &metadata,
    ));

    // TODO: Currently `issuance_event_publishers` and `verification_event_publishers` are exactly the same, which is
    // weird. We need some sort of layer between `agent_application` and `agent_store` that will provide a cleaner way
    // of initializing the event publishers and sending them over to `agent_store`.
    let issuance_event_publishers: Vec<Box<dyn EventPublisher>> = vec![Box::new(EventPublisherHttp::load().unwrap())];
    let verification_event_publishers: Vec<Box<dyn EventPublisher>> =
        vec![Box::new(EventPublisherHttp::load().unwrap())];

    let (issuance_state, verification_state) = match agent_shared::config!("event_store", String).unwrap().as_str() {
        "postgres" => (
            postgres::issuance_state(issuance_event_publishers).await,
            postgres::verification_state(verification_services, verification_event_publishers).await,
        ),
        _ => (
            in_memory::issuance_state(issuance_event_publishers).await,
            in_memory::verification_state(verification_services, verification_event_publishers).await,
        ),
    };

    let url = config!("url", String).expect("AGENT_CONFIG_URL is not set");
    // TODO: Temporary solution. In the future we need to read these kinds of values from a config file.
    std::env::set_var("AGENT_VERIFICATION_URL", &url);

    info!("Application url: {:?}", url);

    let url = url::Url::parse(&url).unwrap();

    initialize(&issuance_state, startup_commands(url.clone(), &metadata)).await;

    let mut app = app((issuance_state, verification_state));

    // CORS
    if config!("enable_cors", bool).unwrap_or(false) {
        info!("CORS (permissive) enabled for all routes");
        app = app.layer(CorsLayer::permissive());
    }

    // did:web
    let enable_did_web = config!("did_method_web_enabled", bool).unwrap_or(false);

    let did_document = if enable_did_web {
        let subject = Subject {
            secret_manager: secret_manager().await,
        };
        Some(
            subject
                .secret_manager
                .produce_document(
                    did_manager::DidMethod::Web,
                    Some(did_manager::MethodSpecificParameters::Web { origin: url.origin() }),
                )
                .await
                .unwrap(),
        )
    } else {
        None
    };

    // Domain Linkage
    let did_configuration_resource = if config!("domain_linkage_enabled", bool).unwrap_or(false) {
        Some(
            create_did_configuration_resource(
                url.clone(),
                did_document
                    .clone()
                    .expect("No DID document found to create a DID Configuration Resource for"),
                secret_manager().await,
            )
            .await
            .expect("Failed to create DID Configuration Resource"),
        )
    } else {
        None
    };

    if let Some(mut did_document) = did_document {
        if let Some(did_configuration_resource) = did_configuration_resource {
            // Create a new service and add it to the DID document.
            let service = Service::builder(Default::default())
                .id(format!("{}#service-1", did_document.id()).parse().unwrap())
                .type_("LinkedDomains")
                .service_endpoint(
                    serde_json::from_value::<ServiceEndpoint>(serde_json::json!(
                        {
                            "origins": [url.origin().ascii_serialization()]
                        }
                    ))
                    .unwrap(),
                )
                .build()
                .expect("Failed to create DID Configuration Resource");
            did_document
                .insert_service(service)
                .expect("Service already exists in DID Document");

            let path = "/.well-known/did-configuration.json";
            info!("Serving DID Configuration (Domain Linkage) at `{path}`");
            app = app.route(path, get(Json(did_configuration_resource)));
        }
        let path = "/.well-known/did.json";
        info!("Serving `did:web` document at `{path}`");
        app = app.route(path, get(Json(did_document)));
    }

    // Health check
    // TODO: reconsider naming this endpoint to `/healthz` or `/livez` according to the Kubernetes conventions.
    // Note: the /health endpoint is added here instead of `agent_api_rest`, because some health checks need to run functions inaccessible in `agent_api_rest`.
    app = app.route("/health", get(health_check));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3033").await.unwrap();
    info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

/// Performs a few low-cost sanity checks to detect possible problems (such as misconfiguration) early.
pub async fn health_check() -> axum::http::StatusCode {
    // Are configured keys accessible?
    let _ = secret_manager().await;

    // Is the event store healthy?
    match agent_shared::config!("event_store", String).unwrap().as_str() {
        "postgres" => {
            // TODO: implement
            // let pool = postgres::default_postgress_pool(&config!("db_connection_string", String).unwrap()).await;
        }
        _ => {
            // TODO: implement
            // Is there enough memory left to store events?
        }
    }

    // TODO: return StatusCode::SERVICE_UNAVAILABLE if any of the checks fail

    StatusCode::OK
}
