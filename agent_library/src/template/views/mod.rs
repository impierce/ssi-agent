pub mod all_templates;

use super::event::TemplateEvent;
use crate::template::aggregate::{Status, Template};
use cqrs_es::{EventEnvelope, View};

pub type TemplateView = Template;

impl View<Template> for Template {
    fn update(&mut self, event: &EventEnvelope<Template>) {
        use TemplateEvent::*;

        match &event.payload {
            TemplateCreated {
                template_id,
                source_template_id,
                title,
                display,
                data_model,
                holder_type,
                modified_at,
                tags,
                status,
                visibility,
                credential_expiration,
                description,
                r#type,
                schema,
                schema_properties_attributes,
                holder_authorization,
            } => {
                self.template_id.clone_from(template_id);
                self.source_template_id.clone_from(source_template_id);
                self.title = title.clone();
                self.display.clone_from(display);
                self.data_model = data_model.clone();
                self.holder_type = holder_type.clone();
                self.modified_at.replace(modified_at.clone());
                self.tags = tags.clone();
                self.status.clone_from(status);
                self.visibility.clone_from(visibility);
                self.credential_expiration = credential_expiration.clone();
                self.description.clone_from(description);
                self.r#type.clone_from(r#type);
                self.schema.clone_from(schema);
                self.schema_properties_attributes
                    .clone_from(schema_properties_attributes);
                self.holder_authorization = holder_authorization.clone();
            }
            TitleUpdated {
                template_id: _,
                title,
                modified_at,
            } => {
                self.title = title.clone();
                self.modified_at.replace(modified_at.clone());
            }
            DisplayUpdated {
                template_id: _,
                display,
                modified_at,
            } => {
                self.display.replace(display.clone());
                self.modified_at.replace(modified_at.clone());
            }
            TagsUpdated {
                template_id: _,
                tags,
                modified_at,
            } => {
                self.tags = if tags.is_empty() { None } else { Some(tags.clone()) };
                self.modified_at.replace(modified_at.clone());
            }
            StatusUpdated {
                template_id: _,
                status,
                modified_at,
            } => {
                self.status.clone_from(status);
                self.modified_at.replace(modified_at.clone());
            }
            VisibilityUpdated {
                template_id: _,
                visibility,
                modified_at,
            } => {
                self.visibility.clone_from(visibility);
                self.modified_at.replace(modified_at.clone());
            }
            DescriptionUpdated {
                template_id: _,
                description,
                modified_at,
            } => {
                self.description = if description.is_empty() {
                    None
                } else {
                    Some(description.clone())
                };
                self.modified_at.replace(modified_at.clone());
            }
            TypeUpdated {
                template_id: _,
                r#type,
                modified_at,
            } => {
                self.r#type.clone_from(r#type);
                self.modified_at.replace(modified_at.clone());
            }
            SchemaUpdated {
                template_id: _,
                schema,
                modified_at,
            } => {
                self.schema.replace(schema.clone());
                self.modified_at.replace(modified_at.clone());
            }
            SchemaPropertiesAttributesUpdated {
                template_id: _,
                schema_properties_attributes,
                modified_at,
            } => {
                self.schema_properties_attributes
                    .replace(schema_properties_attributes.clone());
                self.modified_at.replace(modified_at.clone());
            }
            CredentialExpirationUpdated {
                template_id: _,
                credential_expiration,
                modified_at,
            } => {
                self.credential_expiration = credential_expiration.clone();
                self.modified_at.replace(modified_at.clone());
            }
            HolderAuthorizationUpdated {
                template_id: _,
                holder_authorization,
                modified_at,
            } => {
                self.holder_authorization = holder_authorization.clone();
                self.modified_at.replace(modified_at.clone());
            }
            TemplateDeleted { template_id: _ } => {
                self.status = Status::Deleted;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::aggregate::{DataModel, Visibility};
    use crate::template::event::{Expiration, HolderType};
    use std::collections::HashMap;

    fn event(payload: TemplateEvent) -> EventEnvelope<Template> {
        EventEnvelope {
            aggregate_id: "template-id".to_string(),
            sequence: 1,
            payload,
            metadata: HashMap::new(),
        }
    }

    fn created_event() -> EventEnvelope<Template> {
        event(TemplateEvent::TemplateCreated {
            template_id: "template-id".to_string(),
            source_template_id: None,
            title: "Template".to_string(),
            display: Box::new(None),
            data_model: DataModel::W3CVcDataModelV2_0,
            holder_type: HolderType::Individual,
            modified_at: "2024-01-01T00:00:00Z".to_string(),
            tags: Some(vec!["existing".to_string()]),
            status: Status::Published,
            visibility: Visibility::Private,
            credential_expiration: Expiration::Never,
            description: Some("existing description".to_string()),
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(None),
            schema_properties_attributes: None,
            holder_authorization: agent_shared::config::Authorization::default(),
        })
    }

    #[test]
    fn tags_updated_with_empty_list_clears_tags() {
        let mut view = Template::default();
        view.update(&created_event());

        view.update(&event(TemplateEvent::TagsUpdated {
            template_id: "template-id".to_string(),
            tags: vec![],
            modified_at: "2024-01-02T00:00:00Z".to_string(),
        }));

        assert_eq!(view.tags, None);
        assert_eq!(view.modified_at.as_deref(), Some("2024-01-02T00:00:00Z"));
    }

    #[test]
    fn description_updated_with_empty_string_clears_description() {
        let mut view = Template::default();
        view.update(&created_event());

        view.update(&event(TemplateEvent::DescriptionUpdated {
            template_id: "template-id".to_string(),
            description: String::new(),
            modified_at: "2024-01-02T00:00:00Z".to_string(),
        }));

        assert_eq!(view.description, None);
        assert_eq!(view.modified_at.as_deref(), Some("2024-01-02T00:00:00Z"));
    }

    #[test]
    fn credential_expiration_updated_replaces_previous_value() {
        let mut view = Template::default();
        view.update(&created_event());

        view.update(&event(TemplateEvent::CredentialExpirationUpdated {
            template_id: "template-id".to_string(),
            credential_expiration: Expiration::Duration("P30D".to_string()),
            modified_at: "2024-01-02T00:00:00Z".to_string(),
        }));

        assert_eq!(view.credential_expiration, Expiration::Duration("P30D".to_string()));
        assert_eq!(view.modified_at.as_deref(), Some("2024-01-02T00:00:00Z"));
    }

    #[test]
    fn template_deleted_marks_status_deleted() {
        let mut view = Template::default();
        view.update(&created_event());

        view.update(&event(TemplateEvent::TemplateDeleted {
            template_id: "template-id".to_string(),
        }));

        assert_eq!(view.status, Status::Deleted);
    }
}
