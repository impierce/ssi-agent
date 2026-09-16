use super::TemplateView;
use crate::template::views::Template;
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllTemplatesView {
    #[serde(flatten)]
    pub templates: IndexMap<String, TemplateView>,
}

impl View<Template> for AllTemplatesView {
    fn update(&mut self, event: &EventEnvelope<Template>) {
        self.templates
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
