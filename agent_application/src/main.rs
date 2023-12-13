use agent_api_rest::app;
use agent_issuance::{
    command::IssuanceCommand,
    handlers::command_handler,
    init::load_templates,
    model::aggregate::IssuanceData,
    queries::{IssuanceDataView, SimpleLoggingQuery},
    services::IssuanceServices,
    state::{ApplicationState, CQRS},
};
use agent_shared::config;
use agent_store::{in_memory, postgres};
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
};
use serde_json::json;
use tracing::info;

#[tokio::main]
async fn main() {
    let state = match config!("event_store").unwrap().as_str() {
        "postgres" => postgres::ApplicationState::new(vec![Box::new(SimpleLoggingQuery {})], IssuanceServices {}).await,

        _ => in_memory::ApplicationState::new(vec![Box::new(SimpleLoggingQuery {})], IssuanceServices {}).await,
    };

    match config!("log_format").unwrap().as_str() {
        "json" => tracing_subscriber::fmt().json().init(),
        _ => tracing_subscriber::fmt::init(),
    }

    tokio::spawn(startup_events(state.clone()));

    axum::Server::bind(&"0.0.0.0:3033".parse().unwrap())
        .serve(app(state).into_make_service())
        .await
        .unwrap();
}

async fn startup_events(state: ApplicationState<IssuanceData, IssuanceDataView>) {
    info!("Starting up ...");

    let host = config!("host").unwrap();

    let base_url: url::Url = format!("http://{}:3033/", host).parse().unwrap();

    match command_handler(
        "agg-id-F39A0C".to_string(),
        &state,
        IssuanceCommand::LoadAuthorizationServerMetadata {
            authorization_server_metadata: Box::new(AuthorizationServerMetadata {
                issuer: base_url.clone(),
                token_endpoint: Some(base_url.join("auth/token").unwrap()),
                ..Default::default()
            }),
        },
    )
    .await
    {
        Ok(_) => info!("Startup task completed: `LoadAuthorizationServerMetadata`"),
        Err(err) => println!("Startup task failed: {:#?}", err),
    };

    match command_handler(
        "agg-id-F39A0C".to_string(),
        &state,
        IssuanceCommand::LoadCredentialIssuerMetadata {
            credential_issuer_metadata: CredentialIssuerMetadata {
                credential_issuer: base_url.clone(),
                authorization_server: None,
                credential_endpoint: base_url.join("openid4vci/credential").unwrap(),
                deferred_credential_endpoint: None,
                batch_credential_endpoint: None,
                credentials_supported: vec![],
                display: None,
            },
        },
    )
    .await
    {
        Ok(_) => info!("Startup task completed: `LoadCredentialIssuerMetadata`"),
        Err(err) => println!("Startup task failed: {:#?}", err),
    };

    // Load templates
    load_templates(&state).await;

    match command_handler(
        "agg-id-F39A0C".to_string(),
        &state,
        IssuanceCommand::CreateCredentialsSupported {
            credentials_supported: vec![serde_json::from_value(json!({
                "format": "jwt_vc_json",
                "cryptographic_binding_methods_supported": [
                    "did:key",
                ],
                "cryptographic_suites_supported": [
                    "EdDSA"
                ],
                "credential_definition":{
                    "type": [
                        "VerifiableCredential",
                        "OpenBadgeCredential"
                    ]
                },
                "proof_types_supported": [
                    "jwt"
                ]
            }
            ))
            .unwrap()],
        },
    )
    .await
    {
        Ok(_) => println!("Startup task completed: `CreateCredentialsSupported`"),
        Err(err) => println!("Startup task failed: {:#?}", err),
    };
}
