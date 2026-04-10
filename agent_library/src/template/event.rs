use std::collections::HashMap;

pub use super::aggregate::{DataModel, Display, HolderType, PropertyAttribute, Status, Visibility};
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

// TODO: Add `modified_at` to metadata.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum TemplateEvent {
    TemplateCreated {
        template_id: String,
        source_template_id: Option<String>,
        // TODO: Make this a required field.
        title: Option<String>,
        display: Option<Display>,
        data_model: Option<DataModel>,
        creator: Option<String>,
        holder_type: Option<HolderType>,
        modified_at: String,
        tags: Vec<String>,
        status: Status,
        visibility: Visibility,
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
    DataModelUpdated {
        template_id: String,
        data_model: DataModel,
        modified_at: String,
    },
    CreatorUpdated {
        template_id: String,
        creator: String,
        modified_at: String,
    },
    HolderTypeUpdated {
        template_id: String,
        holder_type: HolderType,
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
    FieldAttributesUpdated {
        template_id: String,
        schema_properties_attributes: HashMap<String, PropertyAttribute>,
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
