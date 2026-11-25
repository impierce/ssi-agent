//! Startup reconciles the persisted organisation profile with the configured display, depending on whether the
//! display is provisioned through configuration, left at its default, or not configured at all.
//!
//! Each file in `tests/` runs in its own process, so changing the global configuration here does not affect other
//! tests. The tests in this file are serialized, since they share that configuration.

use agent_identity::identity_state;
use agent_identity::profile::aggregate::{Profile, Source};
use agent_identity::profile::command::ProfileCommand;
use agent_identity::services::IdentityServices;
use agent_identity::state::{initialize, query_profile, IdentityState, PROFILE_ID};
use agent_shared::config::{config, config_mut, Display, Logo, APPLICATIONCONFIGURATION_PROVISIONING_METADATA};
use agent_shared::handlers::{public_command_handler, public_query_handler};
use agent_store::in_memory::InMemory;
use serial_test::serial;

async fn identity() -> IdentityState {
    identity_state(&InMemory, IdentityServices::default(), &Default::default()).await
}

async fn profile(state: &IdentityState) -> Option<Profile> {
    public_query_handler(PROFILE_ID, &state.query.profile).await.unwrap()
}

async fn execute(state: &IdentityState, command: ProfileCommand) {
    public_command_handler(PROFILE_ID, &state.command.profile, command)
        .await
        .unwrap();
}

fn display() -> Display {
    Display {
        name: "Configured Organisation".to_string(),
        description: Some("Configured description".to_string()),
        locale: None,
        logo: Some(Logo {
            uri: Some("https://example.com/configured-logo.png".parse().unwrap()),
            alt_text: Some("Configured logo".to_string()),
        }),
        country: Some("CH".to_string()),
    }
}

/// Sets the configured display and whether it counts as provisioned, i.e. explicitly set in the configuration
/// rather than left at its default.
fn configure_display(display: Option<Display>, provisioned: bool) {
    // Loading the configuration populates the provisioning metadata, so it has to happen before it is changed below.
    config_mut().display = display.into_iter().collect();

    let mut metadata = APPLICATIONCONFIGURATION_PROVISIONING_METADATA.write().unwrap();
    let metadata = metadata.as_object_mut().unwrap();
    if provisioned {
        metadata.insert("display".to_string(), serde_json::json!([]));
    } else {
        metadata.remove("display");
    }
}

/// A profile as it would have been created from a provisioned display and not changed since.
async fn provisioned_profile(state: &IdentityState) {
    execute(
        state,
        ProfileCommand::CreateProfile {
            profile_id: PROFILE_ID.to_string(),
            display_name: Some("Provisioned Organisation".to_string()),
            description: Some("Provisioned description".to_string()),
            logo: Some(Logo {
                uri: Some("https://example.com/provisioned-logo.png".parse().unwrap()),
                alt_text: None,
            }),
            country: Some("NL".to_string()),
            source: Source::Provisioned,
        },
    )
    .await;
}

fn assert_matches(profile: &Profile, display: &Display, source: Source) {
    assert_eq!(profile.display_name.as_ref(), Some(&display.name));
    assert_eq!(profile.description, display.description);
    assert_eq!(profile.logo, display.logo);
    assert_eq!(profile.country, display.country);
    assert_eq!(profile.source, source);
}

#[tokio::test]
#[serial]
async fn a_default_display_creates_a_default_profile() {
    configure_display(Some(display()), false);
    let state = identity().await;

    initialize(&state).await.unwrap();

    assert_matches(&profile(&state).await.unwrap(), &display(), Source::Default);
}

#[tokio::test]
#[serial]
async fn a_default_display_replaces_a_previously_provisioned_profile() {
    configure_display(Some(display()), false);
    let state = identity().await;
    provisioned_profile(&state).await;

    initialize(&state).await.unwrap();

    assert_matches(&profile(&state).await.unwrap(), &display(), Source::Default);
}

#[tokio::test]
#[serial]
async fn a_default_display_keeps_a_profile_changed_at_runtime() {
    configure_display(Some(display()), false);
    let state = identity().await;
    provisioned_profile(&state).await;
    execute(
        &state,
        ProfileCommand::UpdateSource {
            source: Source::Runtime,
        },
    )
    .await;
    let before = profile(&state).await.unwrap();

    initialize(&state).await.unwrap();

    let after = profile(&state).await.unwrap();
    assert_eq!(after.display_name, before.display_name);
    assert_eq!(after.logo, before.logo);
    assert_eq!(after.source, Source::Runtime);
    // The configured display now reflects the persisted profile.
    assert_eq!(config().display[0].name, "Provisioned Organisation");
}

#[tokio::test]
#[serial]
async fn without_a_configured_display_an_empty_profile_is_created() {
    configure_display(None, false);
    let state = identity().await;

    initialize(&state).await.unwrap();

    let profile = profile(&state).await.unwrap();
    assert_eq!(profile.display_name, None);
    assert_eq!(profile.logo, None);
    assert_eq!(profile.country, None);
    assert_eq!(profile.source, Source::Default);
    // Without a name, the display falls back to the application's hostname.
    let hostname = config().application_url.host_str().unwrap().to_string();
    assert_eq!(config().display[0].name, hostname);
}

#[tokio::test]
#[serial]
async fn removing_a_provisioned_display_clears_the_profile() {
    configure_display(None, false);
    let state = identity().await;
    provisioned_profile(&state).await;

    initialize(&state).await.unwrap();

    let profile = profile(&state).await.unwrap();
    assert_eq!(profile.display_name.as_deref(), Some(""));
    assert_eq!(profile.logo, None);
    assert_eq!(profile.country, None);
}

#[tokio::test]
#[serial]
async fn querying_a_missing_profile_leaves_the_configured_display_unchanged() {
    configure_display(Some(display()), true);
    let state = identity().await;

    query_profile(&state).await.unwrap();

    assert!(profile(&state).await.is_none());
    assert_eq!(config().display[0].name, display().name);
}
