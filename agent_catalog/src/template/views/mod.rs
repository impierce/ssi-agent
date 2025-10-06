pub mod all_templates;

use super::event::TemplateEvent;
use crate::template::aggregate::Template;
use cqrs_es::{EventEnvelope, View};

pub type TemplateView = Template;

impl View<Template> for Template {
    fn update(&mut self, event: &EventEnvelope<Template>) {
        use TemplateEvent::*;

        match &event.payload {
            TemplateCreated { template_id } => {
                self.template_id.clone_from(template_id);
            }
        }
    }
}
