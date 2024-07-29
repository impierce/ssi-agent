#![allow(clippy::await_holding_lock)]

use agent_api_rest::app;
use agent_event_publisher_http::EventPublisherHttp;
use agent_issuance::{startup_commands::startup_commands, state::initialize};
use agent_secret_manager::{secret_manager, subject::Subject};
use agent_shared::{
    config::{config, LogFormat, SupportedDidMethod, ToggleOptions},
    domain_linkage::create_did_configuration_resource,
};
use agent_store::{in_memory, postgres, EventPublisher};
use agent_verification::services::VerificationServices;
use axum::{routing::get, Json};
use identity_document::service::{Service, ServiceEndpoint};
use std::sync::Arc;
use tokio::{fs, io};
use tower_http::cors::CorsLayer;
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

    let verification_services = Arc::new(VerificationServices::new(Arc::new(Subject {
        secret_manager: secret_manager().await,
    })));

    // TODO: Currently `issuance_event_publishers` and `verification_event_publishers` are exactly the same, which is
    // weird. We need some sort of layer between `agent_application` and `agent_store` that will provide a cleaner way
    // of initializing the event publishers and sending them over to `agent_store`.
    let issuance_event_publishers: Vec<Box<dyn EventPublisher>> = vec![Box::new(EventPublisherHttp::load().unwrap())];
    let verification_event_publishers: Vec<Box<dyn EventPublisher>> =
        vec![Box::new(EventPublisherHttp::load().unwrap())];

    let (issuance_state, verification_state) = match agent_shared::config::config().event_store.type_ {
        agent_shared::config::EventStoreType::Postgres => (
            postgres::issuance_state(issuance_event_publishers).await,
            postgres::verification_state(verification_services, verification_event_publishers).await,
        ),
        agent_shared::config::EventStoreType::InMemory => (
            in_memory::issuance_state(issuance_event_publishers).await,
            in_memory::verification_state(verification_services, verification_event_publishers).await,
        ),
    };

    info!("{:?}", config());

    let url = &config().url;

    info!("Application url: {:?}", url);

    let url = url::Url::parse(url).unwrap();

    initialize(&issuance_state, startup_commands(url.clone())).await;

    let mut app = app((issuance_state, verification_state));

    // CORS
    if config().cors_enabled.unwrap_or(false) {
        info!("CORS (permissive) enabled for all routes");
        app = app.layer(CorsLayer::permissive());
    }

    // did:web
    let enable_did_web = config()
        .did_methods
        .get(&SupportedDidMethod::Web)
        .unwrap_or(&ToggleOptions::default())
        .enabled;

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
    let did_configuration_resource = if config().domain_linkage_enabled {
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

    // This is used to indicate that the server accepts requests.
    // In a docker container this file can be searched to see if its ready.
    // A better solution can be made later (needed for impierce-demo)
    fs::create_dir_all("/tmp/unicore/").await?;
    fs::write("/tmp/unicore/accept_requests", []).await?;

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3033").await?;
    info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}
