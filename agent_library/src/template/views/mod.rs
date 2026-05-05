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
                creator,
                holder_type,
                modified_at,
                tags,
                status,
                visibility,
                description,
                r#type,
                schema,
                schema_properties_attributes,
            } => {
                self.template_id.clone_from(template_id);
                self.source_template_id.clone_from(source_template_id);
                self.title = Some(title.clone());
                self.display.clone_from(display);
                self.data_model = Some(data_model.clone());
                self.creator.clone_from(creator);
                self.holder_type = Some(holder_type.clone());
                self.modified_at.replace(modified_at.clone());
                self.tags = tags.clone().unwrap_or_default();
                self.status.clone_from(status);
                self.visibility.clone_from(visibility);
                self.description.clone_from(description);
                self.r#type.clone_from(r#type);
                self.schema.clone_from(schema);
                self.schema_properties_attributes
                    .clone_from(schema_properties_attributes);
            }
            TitleUpdated {
                template_id: _,
                title,
                modified_at,
            } => {
                self.title.replace(title.clone());
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
            CreatorUpdated {
                template_id: _,
                creator,
                modified_at,
            } => {
                self.creator.replace(creator.clone());
                self.modified_at.replace(modified_at.clone());
            }
            TagsUpdated {
                template_id: _,
                tags,
                modified_at,
            } => {
                self.tags.clone_from(tags);
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
                self.description.replace(description.clone());
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
            TemplateDeleted { template_id: _ } => {
                self.status = Status::Deleted;
            }
        }
    }
}
