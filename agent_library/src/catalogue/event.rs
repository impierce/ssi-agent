use crate::catalogue::aggregate::{CatalogueDisplay, CatalogueVisibility};
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Serialize, Display, Deserialize, PartialEq)]
pub enum CatalogueEvent {
    CatalogueCreated {
        id: String,
        display: CatalogueDisplay,
        visibility: CatalogueVisibility,
    },
    CatalogueDisplayUpdated {
        id: String,
        display: CatalogueDisplay,
    },
    VisibilityUpdated {
        id: String,
        visibility: CatalogueVisibility,
    },
    TemplateIdAdded {
        id: String,
        template_id: String,
    },
    TemplateIdRemoved {
        id: String,
        template_id: String,
    },
    CatalogueDeleted {
        id: String,
    },
}

impl DomainEvent for CatalogueEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
