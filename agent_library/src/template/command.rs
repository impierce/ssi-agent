use std::collections::HashMap;

pub use super::aggregate::{DataModel, Display, Expiration, HolderType, PropertyAttribute, Status, Visibility};
use agent_shared::config::Authorization;
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

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
        holder_authorization: Authorization,
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
    UpdateHolderAuthorization {
        template_id: String,
        holder_authorization: Authorization,
    },
    DeleteTemplate {
        template_id: String,
    },
}

impl CommandOperation for TemplateCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateNewTemplate { .. } => "library.templates.create",
            Self::UpdateTitle { .. } => "library.templates.title.update",
            Self::UpdateDisplay { .. } => "library.templates.display.update",
            Self::UpdateTags { .. } => "library.templates.tags.update",
            Self::UpdateStatus { .. } => "library.templates.status.update",
            Self::UpdateVisibility { .. } => "library.templates.visibility.update",
            Self::UpdateDescription { .. } => "library.templates.description.update",
            Self::UpdateType { .. } => "library.templates.types.update",
            Self::UpdateSchema { .. } => "library.templates.schema.update",
            Self::UpdateSchemaPropertiesAttributes { .. } => "library.templates.schema_properties.update",
            Self::UpdateCredentialExpiration { .. } => "library.templates.credential_expiration.update",
            Self::UpdateHolderAuthorization { .. } => "library.templates.holder_authorization.update",
            Self::DeleteTemplate { .. } => "library.templates.delete",
        }
    }
}
