use crate::template::event::{DataModel, Status, TemplateEvent};
use serde::{Deserialize, Serialize};
use shared_kernel::event_bus::CloudEvent;
use shared_kernel::IntegrationEvent;

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
            TemplateEvent::TitleUpdated {
                template_id,
                title,
                ..
            } => Ok(Self::TemplateUpdated {
                template_id,
                title: Some(title),
                status: None,
            }),
            TemplateEvent::StatusUpdated {
                template_id,
                status,
                ..
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

impl TemplateIntegrationEvent {
    /// Translates an internal template domain [`CloudEvent`] to a [`TemplateIntegrationEvent`].
    #[must_use]
    pub fn project_from(domain_ce: &CloudEvent) -> Option<CloudEvent> {
        let data = domain_ce.data.as_ref()?;
        let caller_id = domain_ce.extension.callerid.clone();
        let caller_type = domain_ce.extension.callertype.clone();

        let integration_event = match domain_ce.event_type.as_str() {
            "com.impierce.unicore.template-created" => {
                if let Ok(event) = serde_json::from_value::<TemplateEvent>(serde_json::json!({
                    "TemplateCreated": data
                })) {
                    Self::try_from(event).ok()?
                } else {
                    let template_id = data
                        .get("template_id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .or_else(|| domain_ce.subject.clone())
                        .unwrap_or_default();
                    let title = data
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let data_model = data
                        .get("data_model")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();

                    Self::TemplateCreated {
                        template_id,
                        title,
                        data_model,
                    }
                }
            }
            "com.impierce.unicore.title-updated" => {
                if let Ok(event) = serde_json::from_value::<TemplateEvent>(serde_json::json!({
                    "TitleUpdated": data
                })) {
                    Self::try_from(event).ok()?
                } else {
                    let template_id = data
                        .get("template_id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .or_else(|| domain_ce.subject.clone())
                        .unwrap_or_default();
                    let title = data.get("title").and_then(|v| v.as_str()).map(ToString::to_string);
                    Self::TemplateUpdated {
                        template_id,
                        title,
                        status: None,
                    }
                }
            }
            "com.impierce.unicore.status-updated" => {
                if let Ok(event) = serde_json::from_value::<TemplateEvent>(serde_json::json!({
                    "StatusUpdated": data
                })) {
                    Self::try_from(event).ok()?
                } else {
                    let template_id = data
                        .get("template_id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .or_else(|| domain_ce.subject.clone())
                        .unwrap_or_default();
                    let status = data.get("status").and_then(|v| v.as_str()).map(ToString::to_string);
                    Self::TemplateUpdated {
                        template_id,
                        title: None,
                        status,
                    }
                }
            }
            "com.impierce.unicore.display-updated"
            | "com.impierce.unicore.tags-updated"
            | "com.impierce.unicore.visibility-updated"
            | "com.impierce.unicore.description-updated"
            | "com.impierce.unicore.type-updated" => {
                let template_id = data
                    .get("template_id")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string)
                    .or_else(|| domain_ce.subject.clone())
                    .unwrap_or_default();
                Self::TemplateUpdated {
                    template_id,
                    title: None,
                    status: None,
                }
            }
            _ => return None,
        };

        let mut result_ce = integration_event
            .into_cloud_event("/services/template", caller_id, caller_type)
            .ok()?;

        if let Some(occurred_at) = domain_ce.time {
            result_ce.time = Some(occurred_at);
        }

        Some(result_ce)
    }
}

/// Project an internal domain [`CloudEvent`] to a [`TemplateIntegrationEvent`] if applicable.
#[must_use]
pub fn project_library_event(domain_ce: &CloudEvent) -> Option<CloudEvent> {
    TemplateIntegrationEvent::project_from(domain_ce)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_from_title_updated() {
        let event = TemplateEvent::TitleUpdated {
            template_id: "tmpl-1".to_string(),
            title: "New Title".to_string(),
            modified_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let integration = TemplateIntegrationEvent::try_from(event).expect("should map");
        assert_eq!(
            integration,
            TemplateIntegrationEvent::TemplateUpdated {
                template_id: "tmpl-1".to_string(),
                title: Some("New Title".to_string()),
                status: None,
            }
        );
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
