use agent_api_rest::{app, metrics};
use agent_event_publisher_http::EventPublisherHttp;
use agent_issuance::{startup_commands::startup_commands, state::initialize};
use agent_secret_manager::{secret_manager, subject::Subject};
use agent_shared::config;
use agent_store::{in_memory, postgres, EventPublisher};
use agent_verification::services::VerificationServices;
use axum::Router;
use oid4vc_core::{client_metadata::ClientMetadataResource, SubjectSyntaxType};
use serde_json::json;
use std::{str::FromStr, sync::Arc};
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

    let default_did_method = config!("default_did_method").unwrap_or("did:key".to_string());
    let verification_services = Arc::new(VerificationServices::new(
        Arc::new(Subject {
            secret_manager: secret_manager().await,
        }),
        // TODO: Temporary solution. Remove this once `ClientMetadata` is part of `RelyingPartyManager`.
        ClientMetadataResource::ClientMetadata {
            client_name: None,
            logo_uri: None,
            extension: siopv2::authorization_request::ClientMetadataParameters {
                subject_syntax_types_supported: vec![SubjectSyntaxType::from_str(&default_did_method).unwrap()],
            },
        },
        ClientMetadataResource::ClientMetadata {
            client_name: None,
            logo_uri: None,
            // TODO: fix this once `vp_formats` is public.
            extension: serde_json::from_value(json!({
                "vp_formats": {}
            }))
            .unwrap(),
        },
        &default_did_method,
    ));

    // TODO: Currently `issuance_event_publishers` and `verification_event_publishers` are exactly the same, which is
    // weird. We need some sort of layer between `agent_application` and `agent_store` that will provide a cleaner way
    // of initializing the event publishers and sending them over to `agent_store`.
    let issuance_event_publishers: Vec<Box<dyn EventPublisher>> = vec![Box::new(EventPublisherHttp::load().unwrap())];
    let verification_event_publishers: Vec<Box<dyn EventPublisher>> =
        vec![Box::new(EventPublisherHttp::load().unwrap())];

    let (issuance_state, verification_state) = match agent_shared::config!("event_store").unwrap().as_str() {
        "postgres" => (
            postgres::issuance_state(issuance_event_publishers).await,
            postgres::verification_state(verification_services, verification_event_publishers).await,
        ),
        _ => (
            in_memory::issuance_state(issuance_event_publishers).await,
            in_memory::verification_state(verification_services, verification_event_publishers).await,
        ),
    };

    let url = config!("url").expect("AGENT_APPLICATION_URL is not set");
    // TODO: Temporary solution. In the future we need to read these kinds of values from a config file.
    std::env::set_var("AGENT_VERIFICATION_URL", &url);

    info!("Application url: {:?}", url);

    let url = url::Url::parse(&url).unwrap();

    initialize(&issuance_state, startup_commands(url)).await;

    tokio::join!(
        start_server("app".to_string(), app((issuance_state, verification_state)), 3033),
        // The `/metrics` endpoint should not be publicly available. If behind a reverse proxy, this
        // can be achieved by rejecting requests to `/metrics`. In this example, a second server is
        // started on another port to expose `/metrics`.
        start_server("metrics".to_string(), metrics(), 3031)
    );
}

async fn start_server(alias: String, router: Router, port: u16) {
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await.unwrap();
    info!("`{alias}` server listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, router).await.unwrap();
}
