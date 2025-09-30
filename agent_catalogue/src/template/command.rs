use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TemplateCommand {
    CreateTemplate { template_id: String },
}
