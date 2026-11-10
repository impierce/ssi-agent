use std::collections::HashMap;

pub use super::aggregate::{DataModel, Display, Expiration, HolderType, PropertyAttribute, Status, Visibility};
pub use super::origin::TemplateOrigin;
pub use super::public_content::PublicTemplateContent;
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
        #[serde(default)]
        origin: Option<Box<TemplateOrigin>>,
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
    /// Replaces the template's content with `content` and its origin with `origin` in one step: every field is
    /// validated first, and the events of all fields are committed together or not at all. Only fields that change
    /// emit an event; the origin always does, last. A schema can't be removed, so a `content` without one keeps the
    /// template's. The data model and holder type can't change.
    ///
    /// Before `UpdateOrigin`, which would otherwise match the same fields when deserializing.
    ReplaceContent {
        template_id: String,
        content: Box<PublicTemplateContent>,
        #[serde(deserialize_with = "Option::deserialize")]
        origin: Option<TemplateOrigin>,
    },
    /// Replaces the origin, or removes it with `None`.
    UpdateOrigin {
        template_id: String,
        // Required, though nullable: an absent `origin` must not match this variant, or the
        // untagged deserialization would read `DeleteTemplate` as removing the origin.
        #[serde(deserialize_with = "Option::deserialize")]
        origin: Option<TemplateOrigin>,
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
            Self::ReplaceContent { .. } => "library.templates.content.replace",
            Self::UpdateOrigin { .. } => "library.templates.origin.update",
            Self::DeleteTemplate { .. } => "library.templates.delete",
        }
    }
}
