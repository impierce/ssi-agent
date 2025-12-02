pub use super::aggregate::{CredentialFormat, Display, HolderType, Status, Visibility};
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

// TODO: Add `modified_at` to metadata.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum TemplateEvent {
    TemplateCreated {
        template_id: String,
        title: Option<String>,
        display: Option<Display>,
        credential_format: Option<CredentialFormat>,
        creator: Option<String>,
        holder_type: Option<HolderType>,
        modified_at: String,
        tags: Vec<String>,
        status: Status,
        visibility: Visibility,
        description: Option<String>,
        r#type: Vec<String>,
        schema: Option<serde_json::Value>,
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
    CredentialFormatUpdated {
        template_id: String,
        credential_format: CredentialFormat,
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
}

impl DomainEvent for TemplateEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
