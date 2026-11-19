//! Startup must derive the server metadata from the configuration, and reconcile a persisted server configuration
//! with it on every subsequent start.

use agent_issuance::server_config::command::ServerConfigCommand;
use agent_issuance::server_config::views::ServerConfigView;
use agent_issuance::services::IssuanceServices;
use agent_issuance::state::{initialize, issuance_state, IssuanceState, SERVER_CONFIG_ID};
use agent_secret_manager::service::Service;
use agent_shared::config::{
    config, config_mut, get_all_enabled_did_methods, get_all_enabled_signing_algorithms_supported,
    get_preferred_signing_algorithm, CredentialConfiguration,
};
use agent_shared::handlers::{public_command_handler, public_query_handler};
use agent_shared::UrlAppendHelpers as _;
use agent_store::in_memory::InMemory;
use jsonwebtoken::Algorithm;
use oid4vci::credential_issuer::credential_configurations_supported::AlgIdentifier;
use serde_json::{json, Value};

async fn initialized_state() -> IssuanceState {
    let state = issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await;
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

#[tokio::test]
async fn reinitialization_restores_endpoints_that_point_to_a_previous_issuer_url() {
    let state = initialized_state().await;
    let configured = server_config_json(&state).await;

    let mut drifted = server_config(&state).await;
    drifted.authorization_server_metadata.token_endpoint =
        Some("https://previous-domain.example.org/auth/token".parse().unwrap());
    drifted.credential_issuer_metadata.nonce_endpoint =
        Some("https://previous-domain.example.org/openid4vci/nonce".parse().unwrap());
    public_command_handler(
        SERVER_CONFIG_ID,
        &state.command.server_config,
        ServerConfigCommand::InitializeServerMetadata {
            authorization_server_metadata: Box::new(drifted.authorization_server_metadata),
            credential_issuer_metadata: Box::new(drifted.credential_issuer_metadata),
            cryptographic_binding_methods_supported: drifted.cryptographic_binding_methods_supported,
            signing_algorithms_supported: drifted.signing_algorithms_supported,
        },
    )
    .await
    .unwrap();
    assert_ne!(server_config_json(&state).await, configured);

    initialize(&state).await.unwrap();
    assert_eq!(server_config_json(&state).await, configured);
}

fn set_preferred_signing_algorithm(algorithm: Algorithm) {
    for (enabled_algorithm, options) in config_mut().signing_algorithms_supported.iter_mut() {
        options.preferred = Some(*enabled_algorithm == algorithm);
    }
}

fn advertised_credential_signing_algorithms(server_config: &ServerConfigView) -> Vec<Vec<AlgIdentifier>> {
    let mut advertised: Vec<_> = server_config
        .credential_configurations
        .values()
        .map(|(_provisioned, credential_configuration, _authorization)| {
            credential_configuration.credential_signing_alg_values_supported.clone()
        })
        .collect();
    advertised.extend(
        server_config
            .credential_issuer_metadata
            .credential_configurations_supported
            .values()
            .map(|credential_configuration| credential_configuration.credential_signing_alg_values_supported.clone()),
    );
    advertised
}

#[tokio::test]
#[serial_test::serial]
async fn reinitialization_advertises_only_the_preferred_signing_algorithm_for_credentials() {
    let state = initialized_state().await;
    let preferred = get_preferred_signing_algorithm();
    let previously_preferred = get_all_enabled_signing_algorithms_supported()
        .into_iter()
        .find(|algorithm| *algorithm != preferred)
        .unwrap();

    // A credential configuration added while another algorithm was preferred.
    set_preferred_signing_algorithm(previously_preferred);
    let credential_configuration: CredentialConfiguration = serde_json::from_value(json!({
        "credential_configuration_id": "badge",
        "format": "jwt_vc_json",
        "type": ["VerifiableCredential"],
        "display": [{ "name": "Badge" }]
    }))
    .unwrap();
    let result = public_command_handler(
        SERVER_CONFIG_ID,
        &state.command.server_config,
        ServerConfigCommand::UpdateCredentialConfiguration {
            credential_configuration,
            provisioned: false,
        },
    )
    .await;
    set_preferred_signing_algorithm(preferred);
    result.unwrap();

    let alg = |algorithm: Algorithm| {
        vec![AlgIdentifier::String(
            serde_json::to_value(algorithm).unwrap().as_str().unwrap().to_string(),
        )]
    };
    assert_eq!(
        advertised_credential_signing_algorithms(&server_config(&state).await),
        vec![alg(previously_preferred); 2]
    );

    initialize(&state).await.unwrap();
    assert_eq!(
        advertised_credential_signing_algorithms(&server_config(&state).await),
        vec![alg(preferred); 2]
    );
}
