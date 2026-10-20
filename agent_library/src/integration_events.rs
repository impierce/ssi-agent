use crate::template::aggregate::Template;
use crate::template::event::{DataModel, Status, TemplateEvent};
use cqrs_es::EventEnvelope;
use serde::{Deserialize, Serialize};
use shared_kernel::event_bus::CloudEvent;
use shared_kernel::{IntegrationEvent, ProjectIntegrationEvent};

/// Marker error indicating that a domain event has no public integration event mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnmappedEvent;

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
            Self::TemplateCreated { .. } => "com.impierce.unicore.template.created",
            Self::TemplateUpdated { .. } => "com.impierce.unicore.template.updated",
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

impl TryFrom<TemplateEvent> for TemplateIntegrationEvent {
    type Error = UnmappedEvent;

    fn try_from(event: TemplateEvent) -> Result<Self, Self::Error> {
        match event {
            TemplateEvent::TemplateCreated {
                template_id,
                title,
                data_model,
                ..
            } => {
                let data_model_str = match data_model {
                    DataModel::W3CVcDataModelV1_1 => "w3c_vc_data_model_v1-1",
                    DataModel::W3CVcDataModelV2_0 => "w3c_vc_data_model_v2-0",
                    DataModel::OpenBadges3_0 => "open_badges_3-0",
                    DataModel::EuropeanLearningModelV3_3 => "european_learning_model_v3-3",
                };
                Ok(Self::TemplateCreated {
                    template_id,
                    title,
                    data_model: data_model_str.to_string(),
                })
            }
            TemplateEvent::TitleUpdated { template_id, title, .. } => Ok(Self::TemplateUpdated {
                template_id,
                title: Some(title),
                status: None,
            }),
            TemplateEvent::StatusUpdated {
                template_id, status, ..
            } => {
                let status_str = match status {
                    Status::Draft => "draft",
                    Status::Published => "published",
                    Status::Archived => "archived",
                    Status::Deleted => "deleted",
                };
                Ok(Self::TemplateUpdated {
                    template_id,
                    title: None,
                    status: Some(status_str.to_string()),
                })
            }
            TemplateEvent::DisplayUpdated { template_id, .. }
            | TemplateEvent::TagsUpdated { template_id, .. }
            | TemplateEvent::VisibilityUpdated { template_id, .. }
            | TemplateEvent::DescriptionUpdated { template_id, .. }
            | TemplateEvent::TypeUpdated { template_id, .. } => Ok(Self::TemplateUpdated {
                template_id,
                title: None,
                status: None,
            }),
            _ => Err(UnmappedEvent),
        }
    }
}

impl ProjectIntegrationEvent for Template {
    fn project_integration_event(envelope: &EventEnvelope<Self>) -> Option<CloudEvent> {
        TemplateIntegrationEvent::try_from(envelope.payload.clone())
            .ok()?
            .into_projected_cloud_event(envelope, "/services/template")
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn try_from_title_updated() {
        let event = TemplateEvent::TitleUpdated {
            template_id: "tmpl-1".to_string(),
            title: "New Title".to_string(),
            modified_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let integration = TemplateIntegrationEvent::try_from(event.clone()).expect("should map");
        assert_eq!(
            integration,
            TemplateIntegrationEvent::TemplateUpdated {
                template_id: "tmpl-1".to_string(),
                title: Some("New Title".to_string()),
                status: None,
            }
        );

        let envelope = EventEnvelope {
            aggregate_id: "tmpl-1".to_string(),
            sequence: 2,
            payload: event,
            metadata: HashMap::default(),
        };
        let projected = Template::project_integration_event(&envelope).expect("should project");
        assert_eq!(projected.id, "template:tmpl-1:2");
        assert!(projected.time.is_some());
    }

    #[test]
    fn try_from_status_updated() {
        let event = TemplateEvent::StatusUpdated {
            template_id: "tmpl-1".to_string(),
            status: Status::Published,
            modified_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let integration = TemplateIntegrationEvent::try_from(event).expect("should map");
        assert_eq!(
            integration,
            TemplateIntegrationEvent::TemplateUpdated {
                template_id: "tmpl-1".to_string(),
                title: None,
                status: Some("published".to_string()),
            }
        );
    }
}
