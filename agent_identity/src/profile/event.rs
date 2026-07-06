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
