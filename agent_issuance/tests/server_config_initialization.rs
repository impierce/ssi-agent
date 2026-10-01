//! Startup must derive the server metadata from the configuration, and reconcile a persisted server configuration
//! with it on every subsequent start.

use agent_issuance::server_config::command::ServerConfigCommand;
use agent_issuance::server_config::views::ServerConfigView;
use agent_issuance::services::IssuanceServices;
use agent_issuance::state::{initialize, IssuanceState, SERVER_CONFIG_ID};
use agent_secret_manager::service::Service;
use agent_shared::config::{config, get_all_enabled_did_methods, get_all_enabled_signing_algorithms_supported};
use agent_shared::handlers::{public_command_handler, public_query_handler};
use agent_shared::UrlAppendHelpers as _;
use agent_store::{in_memory::InMemory, issuance_state};
use jsonwebtoken::Algorithm;
use serde_json::Value;

async fn initialized_state() -> IssuanceState {
    let state = issuance_state(
        &InMemory,
        IssuanceServices::default().await,
        &Default::default(),
        Default::default(),
    )
    .await;
    initialize(&state).await.unwrap();

    state
}

async fn server_config(state: &IssuanceState) -> ServerConfigView {
    public_query_handler(SERVER_CONFIG_ID, &state.query.server_config)
        .await
        .unwrap()
        .unwrap()
}

async fn server_config_json(state: &IssuanceState) -> Value {
    serde_json::to_value(server_config(state).await).unwrap()
}

#[tokio::test]
async fn initialization_derives_the_server_metadata_from_the_configuration() {
    let state = initialized_state().await;
    let server_config = server_config(&state).await;
    let public_url = config().public_url.clone();

    let authorization_server_metadata = server_config.authorization_server_metadata;
    assert_eq!(authorization_server_metadata.issuer, public_url);
    assert_eq!(
        authorization_server_metadata.token_endpoint,
        Some(public_url.append_path_segment("auth/token"))
    );
    assert_eq!(
        authorization_server_metadata.pushed_authorization_request_endpoint,
        Some(public_url.append_path_segment("auth/par"))
    );

    let credential_issuer_metadata = server_config.credential_issuer_metadata;
    assert_eq!(credential_issuer_metadata.credential_issuer, public_url);
    assert_eq!(
        credential_issuer_metadata.credential_endpoint,
        public_url.append_path_segment("openid4vci/credential")
    );
    assert_eq!(
        credential_issuer_metadata.display,
        config().display.first().map(|display| vec![serde_json::json!(display)])
    );

    let mut did_methods: Vec<_> = get_all_enabled_did_methods().iter().map(ToString::to_string).collect();
    did_methods.sort();
    assert_eq!(server_config.cryptographic_binding_methods_supported, did_methods);
    assert_eq!(
        server_config.signing_algorithms_supported,
        get_all_enabled_signing_algorithms_supported()
    );

    assert!(format!("{state:?}").starts_with("IssuanceState"));
}

#[tokio::test]
async fn reinitialization_restores_server_metadata_that_drifted_from_the_configuration() {
    let state = initialized_state().await;
    let configured = server_config_json(&state).await;

    for command in [
        ServerConfigCommand::UpdateIssuerUrl {
            url: "https://previous-domain.example.org/".parse().unwrap(),
        },
        ServerConfigCommand::UpdateIssuerDisplay { display: None },
        ServerConfigCommand::UpdateCryptographicBindingMethods {
            cryptographic_binding_methods_supported: vec!["did:example".to_string()],
        },
        ServerConfigCommand::UpdateSigningAlgorithms {
            signing_algorithms_supported: vec![Algorithm::HS256],
        },
    ] {
        public_command_handler(SERVER_CONFIG_ID, &state.command.server_config, command)
            .await
            .unwrap();
    }
    assert_ne!(server_config_json(&state).await, configured);

    initialize(&state).await.unwrap();
    assert_eq!(server_config_json(&state).await, configured);

    // Without any drift, initialization leaves the server metadata unchanged.
    initialize(&state).await.unwrap();
    assert_eq!(server_config_json(&state).await, configured);
}
