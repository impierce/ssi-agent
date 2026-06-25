use std::collections::HashMap;

pub use super::aggregate::{DataModel, Display, Expiration, HolderType, PropertyAttribute, Status, Visibility};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TemplateCommand {
    CreateNewTemplate {
        template_id: String,
        source_template_id: Option<String>,
        title: String,
        display: Box<Option<Display>>,
        data_model: DataModel,
        holder_type: HolderType,
        tags: Option<Vec<String>>,
        status: Status,
        visibility: Visibility,
        credential_expiration: Option<Expiration>,
        description: Option<String>,
        r#type: Vec<String>,
        schema: Box<Option<serde_json::Value>>,
        schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
        pre_authorized: bool,
    },
    UpdateTitle {
        template_id: String,
        title: String,
    },
    UpdateDisplay {
        template_id: String,
        display: Display,
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
    UpdateCredentialExpiration {
        template_id: String,
        credential_expiration: Expiration,
    },
    UpdatePreAuthorized {
        template_id: String,
        pre_authorized: bool,
    },
    DeleteTemplate {
        template_id: String,
    },
}
