use serde::{Deserialize, Serialize};
use shared_kernel::IntegrationEvent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TemplateIntegrationEvent {
    TemplateCreated {
        template_id: String,
        title: String,
        data_model: String,
    },
    TemplateUpdated {
        template_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
}

impl IntegrationEvent for TemplateIntegrationEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::TemplateCreated { .. } => "tech.impierce.unicore.template.created",
            Self::TemplateUpdated { .. } => "tech.impierce.unicore.template.updated",
        }
    }

    fn subject(&self) -> Option<String> {
        match self {
            Self::TemplateCreated { template_id, .. } | Self::TemplateUpdated { template_id, .. } => {
                Some(template_id.clone())
            }
        }
    }
}
