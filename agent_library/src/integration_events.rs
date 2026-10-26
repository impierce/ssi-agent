use crate::template::aggregate::Template;
use crate::template::event::{DataModel, Status, TemplateEvent, Visibility};
use cqrs_es::EventEnvelope;
use serde::{Deserialize, Serialize};
use shared_kernel::event_bus::CloudEvent;
use shared_kernel::{IntegrationEvent, ProjectIntegrationEvent};

/// Public integration events emitted by the credential template library bounded context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, IntegrationEvent)]
#[serde(untagged)]
#[integration_event(source = "/services/template")]
pub enum TemplateIntegrationEvent {
    /// Credential template created in library.
    #[integration_event(rename = "com.impierce.unicore.template.created")]
    TemplateCreated {
        /// Unique identifier of the created template.
        #[integration_event(subject)]
        template_id: String,
        /// Human-readable title of the template.
        title: String,
        /// Credential data model identifier (e.g. `"open_badges_3-0"`, `"w3c_vc_data_model_v2-0"`).
        data_model: String,
    },
    /// Credential template title updated.
    #[integration_event(rename = "com.impierce.unicore.template.title_updated")]
    TitleUpdated {
        /// Unique identifier of the updated template.
        #[integration_event(subject)]
        template_id: String,
        /// Updated human-readable title of the template.
        title: String,
    },
    /// Credential template lifecycle status updated.
    #[integration_event(rename = "com.impierce.unicore.template.status_updated")]
    StatusUpdated {
        /// Unique identifier of the updated template.
        #[integration_event(subject)]
        template_id: String,
        /// Updated lifecycle status (`"draft"`, `"published"`, `"archived"`, or `"deleted"`).
        status: String,
    },
    /// Credential template visibility updated.
    #[integration_event(rename = "com.impierce.unicore.template.visibility_updated")]
    VisibilityUpdated {
        /// Unique identifier of the updated template.
        #[integration_event(subject)]
        template_id: String,
        /// Updated visibility (`"private"` or `"public"`).
        visibility: String,
    },
    /// Credential template display configuration updated.
    #[integration_event(rename = "com.impierce.unicore.template.display_updated")]
    DisplayUpdated {
        /// Unique identifier of the updated template.
        #[integration_event(subject)]
        template_id: String,
    },
    /// Credential template tags updated.
    #[integration_event(rename = "com.impierce.unicore.template.tags_updated")]
    TagsUpdated {
        /// Unique identifier of the updated template.
        #[integration_event(subject)]
        template_id: String,
    },
    /// Credential template description updated.
    #[integration_event(rename = "com.impierce.unicore.template.description_updated")]
    DescriptionUpdated {
        /// Unique identifier of the updated template.
        #[integration_event(subject)]
        template_id: String,
    },
    /// Credential template type values updated.
    #[integration_event(rename = "com.impierce.unicore.template.type_updated")]
    TypeUpdated {
        /// Unique identifier of the updated template.
        #[integration_event(subject)]
        template_id: String,
    },
    /// Credential template JSON schema updated.
    #[integration_event(rename = "com.impierce.unicore.template.schema_updated")]
    SchemaUpdated {
        /// Unique identifier of the updated template.
        #[integration_event(subject)]
        template_id: String,
    },
    /// Credential template schema property attributes updated.
    #[integration_event(rename = "com.impierce.unicore.template.schema_properties_attributes_updated")]
    SchemaPropertiesAttributesUpdated {
        /// Unique identifier of the updated template.
        #[integration_event(subject)]
        template_id: String,
    },
    /// Credential template expiration settings updated.
    #[integration_event(rename = "com.impierce.unicore.template.credential_expiration_updated")]
    CredentialExpirationUpdated {
        /// Unique identifier of the updated template.
        #[integration_event(subject)]
        template_id: String,
    },
    /// Credential template holder authorization settings updated.
    #[integration_event(rename = "com.impierce.unicore.template.holder_authorization_updated")]
    HolderAuthorizationUpdated {
        /// Unique identifier of the updated template.
        #[integration_event(subject)]
        template_id: String,
    },
    /// Credential template deleted from library.
    #[integration_event(rename = "com.impierce.unicore.template.deleted")]
    TemplateDeleted {
        /// Unique identifier of the deleted template.
        #[integration_event(subject)]
        template_id: String,
    },
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
    type IntegrationEvent = TemplateIntegrationEvent;

    fn project_integration_event(envelope: &EventEnvelope<Self>) -> Option<CloudEvent> {
        TemplateIntegrationEvent::from(envelope.payload.clone())
            .into_projected_cloud_event(envelope)
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
        assert!(projected.time.is_none());
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
