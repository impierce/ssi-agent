pub use super::aggregate::{CredentialFormat, Display, HolderType, Status, Visibility};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TemplateCommand {
    CreateTemplate {
        template_id: String,
        duplicate_from: Option<String>,
        title: Option<String>,
        display: Option<Display>,
        credential_format: Option<CredentialFormat>,
        creator: Option<String>,
        holder_type: Option<HolderType>,
        tags: Vec<String>,
        status: Status,
        visibility: Visibility,
        description: Option<String>,
        r#type: Vec<String>,
        schema: Option<serde_json::Value>,
    },
    UpdateTitle {
        template_id: String,
        title: String,
    },
    UpdateDisplay {
        template_id: String,
        display: Display,
    },
    UpdateCredentialFormat {
        template_id: String,
        credential_format: CredentialFormat,
    },
    UpdateCreator {
        template_id: String,
        creator: String,
    },
    UpdateHolderType {
        template_id: String,
        holder_type: HolderType,
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
    DeleteTemplate {
        template_id: String,
    },
    DuplicateTemplate {
        duplicate_from: String,
        new_template_id: String,
    },
}
