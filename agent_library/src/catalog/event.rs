use crate::catalog::aggregate::{CatalogDisplay, CatalogVisibility};
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Serialize, Display, Deserialize, PartialEq)]
pub enum CatalogEvent {
    CatalogCreated {
        id: String,
        display: CatalogDisplay,
        visibility: CatalogVisibility,
    },
    CatalogAppearanceChanged {
        id: String,
        display: CatalogDisplay,
    },
    CatalogMadePublic {
        id: String,
        visibility: CatalogVisibility,
    },
    CatalogMadePrivate {
        id: String,
        visibility: CatalogVisibility,
    },
    TemplateIdsAdded {
        id: String,
        template_ids: Vec<String>,
    },
    TemplateIdsRemoved {
        id: String,
        template_ids: Vec<String>,
    },
    CatalogDeleted {
        id: String,
    },
}

impl DomainEvent for CatalogEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
