use crate::template::aggregate::Template;
use crate::template::event::{DataModel, Status, TemplateEvent, Visibility};
use cqrs_es::EventEnvelope;
use serde::{Deserialize, Serialize};
use shared_kernel::event_bus::CloudEvent;
use shared_kernel::{IntegrationEvent, ProjectIntegrationEvent};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TemplateIntegrationEvent {
    TemplateCreated {
        template_id: String,
        title: String,
        data_model: String,
    },
    TitleUpdated {
        template_id: String,
        title: String,
    },
    StatusUpdated {
        template_id: String,
        status: String,
    },
    VisibilityUpdated {
        template_id: String,
        visibility: String,
    },
    DisplayUpdated {
        template_id: String,
    },
    TagsUpdated {
        template_id: String,
    },
    DescriptionUpdated {
        template_id: String,
    },
    TypeUpdated {
        template_id: String,
    },
    SchemaUpdated {
        template_id: String,
    },
    SchemaPropertiesAttributesUpdated {
        template_id: String,
    },
    CredentialExpirationUpdated {
        template_id: String,
    },
    HolderAuthorizationUpdated {
        template_id: String,
    },
    TemplateDeleted {
        template_id: String,
    },
}

impl IntegrationEvent for TemplateIntegrationEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::TemplateCreated { .. } => "com.impierce.unicore.template.created",
            Self::TitleUpdated { .. } => "com.impierce.unicore.template.title_updated",
            Self::StatusUpdated { .. } => "com.impierce.unicore.template.status_updated",
            Self::VisibilityUpdated { .. } => "com.impierce.unicore.template.visibility_updated",
            Self::DisplayUpdated { .. } => "com.impierce.unicore.template.display_updated",
            Self::TagsUpdated { .. } => "com.impierce.unicore.template.tags_updated",
            Self::DescriptionUpdated { .. } => "com.impierce.unicore.template.description_updated",
            Self::TypeUpdated { .. } => "com.impierce.unicore.template.type_updated",
            Self::SchemaUpdated { .. } => "com.impierce.unicore.template.schema_updated",
            Self::SchemaPropertiesAttributesUpdated { .. } => {
                "com.impierce.unicore.template.schema_properties_attributes_updated"
            }
            Self::CredentialExpirationUpdated { .. } => "com.impierce.unicore.template.credential_expiration_updated",
            Self::HolderAuthorizationUpdated { .. } => "com.impierce.unicore.template.holder_authorization_updated",
            Self::TemplateDeleted { .. } => "com.impierce.unicore.template.deleted",
        }
    }

    fn subject(&self) -> Option<String> {
        let template_id = match self {
            Self::TemplateCreated { template_id, .. }
            | Self::TitleUpdated { template_id, .. }
            | Self::StatusUpdated { template_id, .. }
            | Self::VisibilityUpdated { template_id, .. }
            | Self::DisplayUpdated { template_id }
            | Self::TagsUpdated { template_id }
            | Self::DescriptionUpdated { template_id }
            | Self::TypeUpdated { template_id }
            | Self::SchemaUpdated { template_id }
            | Self::SchemaPropertiesAttributesUpdated { template_id }
            | Self::CredentialExpirationUpdated { template_id }
            | Self::HolderAuthorizationUpdated { template_id }
            | Self::TemplateDeleted { template_id } => template_id,
        };
        Some(template_id.clone())
    }
}

impl From<TemplateEvent> for TemplateIntegrationEvent {
    fn from(event: TemplateEvent) -> Self {
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
                Self::TemplateCreated {
                    template_id,
                    title,
                    data_model: data_model_str.to_string(),
                }
            }
            TemplateEvent::TitleUpdated { template_id, title, .. } => Self::TitleUpdated { template_id, title },
            TemplateEvent::StatusUpdated {
                template_id, status, ..
            } => {
                let status_str = match status {
                    Status::Draft => "draft",
                    Status::Published => "published",
                    Status::Archived => "archived",
                    Status::Deleted => "deleted",
                };
                Self::StatusUpdated {
                    template_id,
                    status: status_str.to_string(),
                }
            }
            TemplateEvent::VisibilityUpdated {
                template_id,
                visibility,
                ..
            } => {
                let visibility_str = match visibility {
                    Visibility::Private => "private",
                    Visibility::Public => "public",
                };
                Self::VisibilityUpdated {
                    template_id,
                    visibility: visibility_str.to_string(),
                }
            }
            TemplateEvent::DisplayUpdated { template_id, .. } => Self::DisplayUpdated { template_id },
            TemplateEvent::TagsUpdated { template_id, .. } => Self::TagsUpdated { template_id },
            TemplateEvent::DescriptionUpdated { template_id, .. } => Self::DescriptionUpdated { template_id },
            TemplateEvent::TypeUpdated { template_id, .. } => Self::TypeUpdated { template_id },
            TemplateEvent::SchemaUpdated { template_id, .. } => Self::SchemaUpdated { template_id },
            TemplateEvent::SchemaPropertiesAttributesUpdated { template_id, .. } => {
                Self::SchemaPropertiesAttributesUpdated { template_id }
            }
            TemplateEvent::CredentialExpirationUpdated { template_id, .. } => {
                Self::CredentialExpirationUpdated { template_id }
            }
            TemplateEvent::HolderAuthorizationUpdated { template_id, .. } => {
                Self::HolderAuthorizationUpdated { template_id }
            }
            TemplateEvent::TemplateDeleted { template_id } => Self::TemplateDeleted { template_id },
        }
    }
}

impl ProjectIntegrationEvent for Template {
    fn project_integration_event(envelope: &EventEnvelope<Self>) -> Option<CloudEvent> {
        TemplateIntegrationEvent::from(envelope.payload.clone())
            .into_projected_cloud_event(envelope, "/services/template")
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn from_title_updated() {
        let event = TemplateEvent::TitleUpdated {
            template_id: "tmpl-1".to_string(),
            title: "New Title".to_string(),
            modified_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let integration = TemplateIntegrationEvent::from(event.clone());
        assert_eq!(
            integration,
            TemplateIntegrationEvent::TitleUpdated {
                template_id: "tmpl-1".to_string(),
                title: "New Title".to_string(),
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
        assert_eq!(projected.event_type, "com.impierce.unicore.template.title_updated");
        assert!(projected.time.is_some());
    }

    #[test]
    fn from_status_updated() {
        let event = TemplateEvent::StatusUpdated {
            template_id: "tmpl-1".to_string(),
            status: Status::Published,
            modified_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let integration = TemplateIntegrationEvent::from(event);
        assert_eq!(
            integration,
            TemplateIntegrationEvent::StatusUpdated {
                template_id: "tmpl-1".to_string(),
                status: "published".to_string(),
            }
        );
    }
}
