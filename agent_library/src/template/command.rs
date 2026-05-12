use std::collections::HashMap;

pub use super::aggregate::{DataModel, Display, Expiration, HolderType, PropertyAttribute, Status, Visibility};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TemplateCommand {
    CreateTemplate {
        template_id: String,
        source_template_id: Option<String>,
        title: String,
        display: Box<Option<Display>>,
        data_model: DataModel,
        creator: Option<String>,
        holder_type: HolderType,
        tags: Option<Vec<String>>,
        status: Status,
        visibility: Visibility,
        expiration: Option<Expiration>,
        description: Option<String>,
        r#type: Vec<String>,
        schema: Box<Option<serde_json::Value>>,
        schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
    },
    UpdateTitle {
        template_id: String,
        title: String,
    },
    UpdateDisplay {
        template_id: String,
        display: Display,
    },
    UpdateCreator {
        template_id: String,
        creator: String,
    },
    UpdateTags {
        template_id: String,
        tags: Vec<String>,
    },
    UpdateStatus {
        template_id: String,
        status: Status,
    },
    UpdateVisibility {
        template_id: String,
        visibility: Visibility,
    },
    UpdateDescription {
        template_id: String,
        description: String,
    },
    UpdateType {
        template_id: String,
        r#type: Vec<String>,
    },
    UpdateSchema {
        template_id: String,
        schema: serde_json::Value,
    },
    UpdateSchemaPropertiesAttributes {
        template_id: String,
        schema_properties_attributes: HashMap<String, PropertyAttribute>,
    },
    UpdateExpiration {
        template_id: String,
        expiration: Expiration,
    },
    DeleteTemplate {
        template_id: String,
    },
}
