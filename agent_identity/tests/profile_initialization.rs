//! Startup must create the organisation profile from the configured display, and keep a profile that was
//! provisioned through configuration in sync with it.

use agent_identity::identity_state;
use agent_identity::profile::aggregate::{Profile, Source};
use agent_identity::profile::command::ProfileCommand;
use agent_identity::services::IdentityServices;
use agent_identity::state::{initialize, IdentityState, PROFILE_ID};
use agent_shared::config::{config, Logo};
use agent_shared::handlers::{public_command_handler, public_query_handler};
use agent_store::in_memory::InMemory;

async fn identity() -> IdentityState {
    identity_state(&InMemory, IdentityServices::default(), &Default::default()).await
}

async fn profile(state: &IdentityState) -> Profile {
    public_query_handler(PROFILE_ID, &state.query.profile)
        .await
        .unwrap()
        .unwrap()
}

/// The test configuration defines its display in the configuration file, so it counts as provisioned.
fn assert_matches_configured_display(profile: &Profile) {
    let display = config().display.first().cloned().unwrap();

    assert_eq!(profile.display_name.as_ref(), Some(&display.name));
    assert_eq!(profile.description, display.description);
    assert_eq!(profile.logo, display.logo);
    assert_eq!(profile.country, display.country);
    assert_eq!(profile.source, Source::Provisioned);
}

#[tokio::test]
async fn initialization_creates_the_profile_from_the_configured_display() {
    let state = identity().await;

    initialize(&state).await.unwrap();

    assert_matches_configured_display(&profile(&state).await);
}

#[tokio::test]
async fn initialization_restores_a_profile_that_drifted_from_the_configured_display() {
    let state = identity().await;
    initialize(&state).await.unwrap();

    for command in [
        ProfileCommand::UpdateSource {
            source: Source::Runtime,
        },
        ProfileCommand::UpdateDisplayName {
            display_name: "Renamed Organisation".to_string(),
            source: Source::Runtime,
        },
        ProfileCommand::UpdateDescription {
            description: Some("Changed at runtime".to_string()),
            source: Source::Runtime,
        },
        ProfileCommand::UpdateLogo {
            logo: Some(Logo {
                uri: Some("https://example.com/other-logo.png".parse().unwrap()),
                alt_text: None,
            }),
            source: Source::Runtime,
        },
        ProfileCommand::UpdateCountry {
            country: Some("NL".to_string()),
            source: Source::Runtime,
        },
    ] {
        public_command_handler(PROFILE_ID, &state.command.profile, command)
            .await
            .unwrap();
    }
    assert_eq!(profile(&state).await.source, Source::Runtime);

    initialize(&state).await.unwrap();

    assert_matches_configured_display(&profile(&state).await);
}
