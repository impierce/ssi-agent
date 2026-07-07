use agent_shared::config::Logo;
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

use crate::profile::aggregate::Source;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum ProfileEvent {
    ProfileCreated {
        profile_id: String,
        display_name: Option<String>,
        description: Option<String>,
        logo: Option<Logo>,
        country: Option<String>,
        source: Source,
    },
    DisplayNameUpdated {
        display_name: String,
        source: Source,
    },
    DescriptionUpdated {
        description: Option<String>,
        source: Source,
    },
    LogoUpdated {
        logo: Option<Logo>,
        source: Source,
    },
    CountryUpdated {
        country: Option<String>,
        source: Source,
    },
    SourceUpdated {
        source: Source,
    },
}

impl DomainEvent for ProfileEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    // Integer schema version of this event payload. Bump on breaking change and add an upcaster (see docs/event-versioning.md).
    fn event_version(&self) -> String {
        "1".to_string()
    }
}

/// Upcasters migrating old persisted versions of these events to the current
/// schema version. See `docs/event-versioning.md`.
pub fn upcasters() -> Vec<Box<dyn cqrs_es::persist::EventUpcaster>> {
    vec![]
}

#[cfg(test)]
mod event_tests {
    use super::*;

    fn logo() -> Logo {
        Logo {
            uri: Some("https://example.com/logo.png".parse().unwrap()),
            alt_text: Some("Organisational Logo".to_string()),
        }
    }

    fn all_variants() -> Vec<ProfileEvent> {
        vec![
            ProfileEvent::ProfileCreated {
                profile_id: "profile-1".to_string(),
                display_name: Some("Time Regulation Institute".to_string()),
                description: Some("A description".to_string()),
                logo: Some(logo()),
                country: Some("NL".to_string()),
                source: Source::Provisioned,
            },
            ProfileEvent::DisplayNameUpdated {
                display_name: "Timeless Institute".to_string(),
                source: Source::Runtime,
            },
            ProfileEvent::DescriptionUpdated {
                description: Some("An updated description".to_string()),
                source: Source::Runtime,
            },
            ProfileEvent::LogoUpdated {
                logo: Some(logo()),
                source: Source::Runtime,
            },
            ProfileEvent::CountryUpdated {
                country: Some("DE".to_string()),
                source: Source::Runtime,
            },
            ProfileEvent::SourceUpdated {
                source: Source::Default,
            },
        ]
    }

    #[test]
    fn round_trips_every_variant() {
        for event in all_variants() {
            let value = serde_json::to_value(&event).unwrap();
            let deserialized: ProfileEvent = serde_json::from_value(value).unwrap();
            assert_eq!(event, deserialized);
        }
    }

    #[test]
    fn golden_profile_created() {
        let golden = serde_json::json!({
            "ProfileCreated": {
                "profile_id": "profile-1",
                "display_name": "Time Regulation Institute",
                "description": "A description",
                "logo": {
                    "uri": "https://example.com/logo.png",
                    "alt_text": "Organisational Logo"
                },
                "country": "NL",
                "source": "Provisioned"
            }
        });

        let event: ProfileEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_display_name_updated() {
        let golden = serde_json::json!({
            "DisplayNameUpdated": {
                "display_name": "Timeless Institute",
                "source": "Runtime"
            }
        });

        let event: ProfileEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_description_updated() {
        let golden = serde_json::json!({
            "DescriptionUpdated": {
                "description": "An updated description",
                "source": "Runtime"
            }
        });

        let event: ProfileEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_logo_updated() {
        let golden = serde_json::json!({
            "LogoUpdated": {
                "logo": {
                    "uri": "https://example.com/logo.png",
                    "alt_text": "Organisational Logo"
                },
                "source": "Runtime"
            }
        });

        let event: ProfileEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_country_updated() {
        let golden = serde_json::json!({
            "CountryUpdated": {
                "country": "DE",
                "source": "Runtime"
            }
        });

        let event: ProfileEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_source_updated() {
        let golden = serde_json::json!({
            "SourceUpdated": {
                "source": "Default"
            }
        });

        let event: ProfileEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }
}
