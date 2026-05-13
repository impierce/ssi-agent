use agent_identity::{
    profile::{aggregate::Source, command::ProfileCommand},
    service::command::ServiceCommand,
    services::IdentityServices,
};
use agent_issuance::services::IssuanceServices;
use agent_secret_manager::service::Service;
use agent_shared::{
    config::{config, set_config, Display, Logo, SupportedDidMethod, ToggleOptions},
    handlers::command_handler,
};
use agent_store::{identity_state, in_memory::InMemory, issuance_state};
use jsonwebtoken::Algorithm;
use std::{fs, time::SystemTime};

fn restore_config(original: agent_shared::config::ApplicationConfiguration) {
    *set_config() = original;
}

#[serial_test::serial]
#[tokio::test]
async fn issuance_startup_update_paths_dispatch_commands() {
    let original = config().clone();
    let state = issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await;
    agent_issuance::state::initialize(&state).await.unwrap();

    set_config().public_url = "https://updated.example.org/".parse().unwrap();
    agent_issuance::state::load_server_metadata(&state).await.unwrap();

    set_config().display[0].name = "Updated Issuer".to_string();
    agent_issuance::state::load_server_metadata(&state).await.unwrap();

    set_config().disable_did_method(SupportedDidMethod::Key);
    agent_issuance::state::update_cryptographic_binding_methods(&state)
        .await
        .unwrap();

    set_config()
        .signing_algorithms_supported
        .get_mut(&Algorithm::ES256)
        .unwrap()
        .enabled = false;
    agent_issuance::state::update_signing_algorithms(&state).await.unwrap();

    restore_config(original);
}

#[serial_test::serial]
#[tokio::test]
async fn issuance_removes_stale_provisioned_credential_configurations() {
    let original = config().clone();
    set_config().credential_configuration_file = None;

    let state = issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await;
    agent_issuance::state::initialize(&state).await.unwrap();

    let path = std::env::temp_dir().join(format!(
        "empty-credential-configurations-{:?}.json",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&path, "[]").unwrap();
    set_config().credential_configuration_file = Some(Box::new(path.clone()));

    agent_issuance::state::update_credential_configurations(&state)
        .await
        .unwrap();

    let _ = fs::remove_file(path);
    restore_config(original);
}

#[serial_test::serial]
#[tokio::test]
async fn identity_initialize_updates_provisioned_profile_fields() {
    let original = config().clone();
    let state = identity_state(&InMemory, IdentityServices::default(), Default::default()).await;

    command_handler(
        state.authorization_checker.clone(),
        None,
        agent_identity::state::PROFILE_ID,
        &state.command.profile,
        ProfileCommand::CreateProfile {
            profile_id: agent_identity::state::PROFILE_ID.to_string(),
            display_name: Some("Old Name".to_string()),
            description: Some("Old Description".to_string()),
            logo: Some(Logo {
                uri: Some("https://old.example.org/logo.png".parse().unwrap()),
                alt_text: Some("Old logo".to_string()),
            }),
            country: Some("BE".to_string()),
            source: Source::Provisioned,
        },
    )
    .await
    .unwrap();

    set_config().display = vec![Display {
        name: "New Name".to_string(),
        description: Some("New Description".to_string()),
        logo: Some(Logo {
            uri: Some("https://new.example.org/logo.png".parse().unwrap()),
            alt_text: Some("New logo".to_string()),
        }),
        country: Some("NL".to_string()),
        ..Default::default()
    }];

    agent_identity::state::initialize(&state).await.unwrap();
    restore_config(original);
}

#[serial_test::serial]
#[tokio::test]
async fn identity_initialize_handles_empty_display_configuration() {
    let original = config().clone();
    let state = identity_state(&InMemory, IdentityServices::default(), Default::default()).await;

    command_handler(
        state.authorization_checker.clone(),
        None,
        agent_identity::state::PROFILE_ID,
        &state.command.profile,
        ProfileCommand::CreateProfile {
            profile_id: agent_identity::state::PROFILE_ID.to_string(),
            display_name: Some("Provisioned Name".to_string()),
            description: None,
            logo: Some(Logo {
                uri: Some("https://old.example.org/logo.png".parse().unwrap()),
                alt_text: Some("Old logo".to_string()),
            }),
            country: Some("BE".to_string()),
            source: Source::Provisioned,
        },
    )
    .await
    .unwrap();

    set_config().display = vec![];
    agent_identity::state::initialize(&state).await.unwrap();

    let empty_state = identity_state(&InMemory, IdentityServices::default(), Default::default()).await;
    agent_identity::state::initialize(&empty_state).await.unwrap();

    restore_config(original);
}

#[serial_test::serial]
#[tokio::test]
async fn identity_document_and_service_initialization_dispatch_commands() {
    let original = config().clone();
    set_config().did_methods.insert(
        SupportedDidMethod::Web,
        ToggleOptions {
            enabled: true,
            preferred: None,
        },
    );
    set_config().domain_linkage_enabled = true;

    let state = identity_state(&InMemory, IdentityServices::default(), Default::default()).await;
    agent_identity::state::initialize(&state).await.unwrap();

    command_handler(
        state.authorization_checker.clone(),
        None,
        agent_identity::state::LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID,
        &state.command.service,
        ServiceCommand::CreateLinkedVerifiablePresentationService {
            service_id: agent_identity::state::LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.to_string(),
            presentation_ids: vec!["presentation-id".to_string()],
        },
    )
    .await
    .unwrap();
    agent_identity::state::initialize_linked_verifiable_presentations(&state)
        .await
        .unwrap();

    set_config().domain_linkage_enabled = false;
    agent_identity::state::initialize_domain_linkage(&state).await.unwrap();

    restore_config(original);
}
