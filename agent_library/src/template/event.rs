use std::collections::HashMap;

pub use super::aggregate::{DataModel, Display, Expiration, HolderType, PropertyAttribute, Status, Visibility};
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

// TODO: Add `modified_at` to metadata.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum TemplateEvent {
    TemplateCreated {
        template_id: String,
        source_template_id: Option<String>,
        title: String,
        display: Box<Option<Display>>,
        data_model: DataModel,
        creator: Option<String>,
        holder_type: HolderType,
        modified_at: String,
        tags: Option<Vec<String>>,
        status: Status,
        visibility: Visibility,
        expiration: Expiration,
        description: Option<String>,
        r#type: Vec<String>,
        schema: Box<Option<serde_json::Value>>,
        schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
    },
    TitleUpdated {
        template_id: String,
        title: String,
        modified_at: String,
    },
    DisplayUpdated {
        template_id: String,
        display: Display,
        modified_at: String,
    },
    CreatorUpdated {
        template_id: String,
        creator: String,
        modified_at: String,
    },
    TagsUpdated {
        template_id: String,
        tags: Vec<String>,
        modified_at: String,
    },
    StatusUpdated {
        template_id: String,
        status: Status,
        modified_at: String,
    },
    VisibilityUpdated {
        template_id: String,
        visibility: Visibility,
        modified_at: String,
    },
    DescriptionUpdated {
        template_id: String,
        description: String,
        modified_at: String,
    },
    TypeUpdated {
        template_id: String,
        r#type: Vec<String>,
        modified_at: String,
    },
    SchemaUpdated {
        template_id: String,
        schema: serde_json::Value,
        modified_at: String,
    },
    SchemaPropertiesAttributesUpdated {
        template_id: String,
        schema_properties_attributes: HashMap<String, PropertyAttribute>,
        modified_at: String,
    },
    ExpirationUpdated {
        template_id: String,
        expiration: Expiration,
        modified_at: String,
    },
    TemplateDeleted {
        template_id: String,
    },
}

impl DomainEvent for TemplateEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
