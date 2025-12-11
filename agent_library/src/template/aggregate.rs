use std::pin::Pin;

use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tracing::{debug, info};

use super::{command::TemplateCommand, error::TemplateError, event::TemplateEvent};

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Logo {
    pub uri: String,
    pub alt_text: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Display {
    pub name: String,
    pub logo: Option<Logo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum CredentialFormat {
    // See https://www.w3.org/TR/vc-data-model-1.1/
    #[serde(rename = "w3c_vc_data_model_v1-1")]
    W3CVcDataModelV11,
    // See https://www.w3.org/TR/vc-data-model-2.0/
    #[serde(rename = "w3c_vc_data_model_v2-0")]
    W3CVcDataModelV20,
    // See https://www.imsglobal.org/spec/ob/v3p0/
    #[serde(rename = "open_badges_3-0")]
    OpenBadges30,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HolderType {
    Individual,
    Organization,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Draft,
    Published,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    #[default]
    Private,
    Public,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Template {
    #[serde(rename = "id")]
    pub template_id: String,
    pub title: Option<String>,
    pub display: Option<Display>,
    pub credential_format: Option<CredentialFormat>,
    pub creator: Option<String>,
    pub holder_type: Option<HolderType>,
    pub modified_at: Option<String>,
    pub tags: Vec<String>,
    pub status: Status,
    pub require_pin_code: Option<bool>,
    pub visibility: Visibility,
    pub description: Option<String>,
    pub r#type: Vec<String>,
    pub schema: Option<serde_json::Value>,
}

#[async_trait]
impl Aggregate for Template {
    type Command = TemplateCommand;
    type Event = TemplateEvent;
    type Error = TemplateError;
    type Services = ();

    fn aggregate_type() -> String {
        "template".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use TemplateCommand::*;
        use TemplateEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateTemplate {
                template_id,
                title,
                display,
                credential_format,
                creator,
                holder_type,
                tags,
                status,
                require_pin_code,
                visibility,
                description,
                r#type,
                schema,
            } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![TemplateCreated {
                    template_id,
                    title,
                    display,
                    credential_format,
                    creator,
                    holder_type,
                    modified_at,
                    tags,
                    status,
                    require_pin_code,
                    visibility,
                    description,
                    r#type,
                    schema,
                }])
            }
            UpdateTitle { template_id, title } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![TitleUpdated {
                    template_id,
                    title,
                    modified_at,
                }])
            }
            UpdateDisplay { template_id, display } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![DisplayUpdated {
                    template_id,
                    display,
                    modified_at,
                }])
            }
            UpdateCredentialFormat {
                template_id,
                credential_format,
            } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![CredentialFormatUpdated {
                    template_id,
                    credential_format,
                    modified_at,
                }])
            }
            UpdateCreator { template_id, creator } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![CreatorUpdated {
                    template_id,
                    creator,
                    modified_at,
                }])
            }
            UpdateHolderType {
                template_id,
                holder_type,
            } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![HolderTypeUpdated {
                    template_id,
                    holder_type,
                    modified_at,
                }])
            }
            UpdateTags { template_id, tags } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![TagsUpdated {
                    template_id,
                    tags,
                    modified_at,
                }])
            }
            UpdateStatus { template_id, status } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![StatusUpdated {
                    template_id,
                    status,
                    modified_at,
                }])
            }
            UpdateVisibility {
                template_id,
                visibility,
            } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![VisibilityUpdated {
                    template_id,
                    visibility,
                    modified_at,
                }])
            }
            UpdateDescription {
                template_id,
                description,
            } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![DescriptionUpdated {
                    template_id,
                    description,
                    modified_at,
                }])
            }
            UpdateType { template_id, r#type } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![TypeUpdated {
                    template_id,
                    r#type,
                    modified_at,
                }])
            }
            UpdateSchema { template_id, schema } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![SchemaUpdated {
                    template_id,
                    schema,
                    modified_at,
                }])
            }
            RequirePinCode {
                template_id,
                require_pin_code,
            } => {
                #[cfg(not(test))]
                let modified_at = chrono::Utc::now().to_rfc3339();
                #[cfg(test)]
                let modified_at = test_utils::modified_at();

                Ok(vec![PinCodeRequired {
                    template_id,
                    require_pin_code,
                    modified_at,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use TemplateEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            TemplateCreated {
                template_id,
                title,
                display,
                credential_format,
                creator,
                holder_type,
                modified_at,
                tags,
                status,
                require_pin_code,
                visibility,
                description,
                r#type,
                schema,
            } => {
                self.template_id = template_id;
                self.title = title;
                self.display = display;
                self.credential_format = credential_format;
                self.creator = creator;
                self.holder_type = holder_type;
                self.modified_at.replace(modified_at);
                self.tags = tags;
                self.status = status;
                self.require_pin_code = require_pin_code;
                self.visibility = visibility;
                self.description = description;
                self.r#type = r#type;
                self.schema = schema;
            }
            TitleUpdated {
                template_id: _,
                title,
                modified_at,
            } => {
                self.title = Some(title);
                self.modified_at.replace(modified_at);
            }
            DisplayUpdated {
                template_id: _,
                display,
                modified_at,
            } => {
                self.display = Some(display);
                self.modified_at.replace(modified_at);
            }
            CredentialFormatUpdated {
                template_id: _,
                credential_format,
                modified_at,
            } => {
                self.credential_format = Some(credential_format);
                self.modified_at.replace(modified_at);
            }
            CreatorUpdated {
                template_id: _,
                creator,
                modified_at,
            } => {
                self.creator = Some(creator);
                self.modified_at.replace(modified_at);
            }
            HolderTypeUpdated {
                template_id: _,
                holder_type,
                modified_at,
            } => {
                self.holder_type = Some(holder_type);
                self.modified_at.replace(modified_at);
            }
            TagsUpdated {
                template_id: _,
                tags,
                modified_at,
            } => {
                self.tags = tags;
                self.modified_at.replace(modified_at);
            }
            StatusUpdated {
                template_id: _,
                status,
                modified_at,
            } => {
                self.status = status;
                self.modified_at.replace(modified_at);
            }
            VisibilityUpdated {
                template_id: _,
                visibility,
                modified_at,
            } => {
                self.visibility = visibility;
                self.modified_at.replace(modified_at);
            }
            DescriptionUpdated {
                template_id: _,
                description,
                modified_at,
            } => {
                self.description = Some(description);
                self.modified_at.replace(modified_at);
            }
            TypeUpdated {
                template_id: _,
                r#type,
                modified_at,
            } => {
                self.r#type = r#type;
                self.modified_at.replace(modified_at);
            }
            SchemaUpdated {
                template_id: _,
                schema,
                modified_at,
            } => {
                self.schema = Some(schema);
                self.modified_at.replace(modified_at);
            }
            PinCodeRequired {
                template_id: _,
                require_pin_code,
                modified_at,
            } => {
                self.require_pin_code = Some(require_pin_code);
                self.modified_at.replace(modified_at);
            }
        }
    }
}

#[cfg(test)]
pub mod document_tests {
    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type TemplateTestFramework = TestFramework<Template>;

    #[allow(clippy::too_many_arguments)]
    #[rstest]
    #[serial_test::serial]
    async fn test_create_template(
        template_id: String,
        title: Option<String>,
        display: Option<Display>,
        credential_format: Option<CredentialFormat>,
        creator: Option<String>,
        holder_type: Option<HolderType>,
        modified_at: String,
        tags: Vec<String>,
        status: Status,
        require_pin_code: Option<bool>,
        visibility: Visibility,
        description: Option<String>,
        r#type: Vec<String>,
        schema: Option<serde_json::Value>,
    ) {
        TemplateTestFramework::with(())
            .given_no_previous_events()
            .when(TemplateCommand::CreateTemplate {
                template_id: template_id.clone(),
                title: title.clone(),
                display: display.clone(),
                credential_format: credential_format.clone(),
                creator: creator.clone(),
                holder_type: holder_type.clone(),
                tags: tags.clone(),
                status: status.clone(),
                require_pin_code: require_pin_code.clone(),
                visibility: visibility.clone(),
                description: description.clone(),
                r#type: r#type.clone(),
                schema: schema.clone(),
            })
            .then_expect_events(vec![TemplateEvent::TemplateCreated {
                template_id,
                title,
                display,
                credential_format,
                creator,
                holder_type,
                modified_at,
                tags,
                status,
                require_pin_code,
                visibility,
                description,
                r#type,
                schema,
            }])
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use rstest::fixture;

    #[fixture]
    pub fn template_id() -> String {
        "template_id".to_string()
    }

    #[fixture]
    pub fn title() -> Option<String> {
        Some("Sample Template".to_string())
    }

    #[fixture]
    pub fn display() -> Option<Display> {
        Some(Display {
            name: "Sample Display".to_string(),
            logo: None,
        })
    }

    #[fixture]
    pub fn credential_format() -> Option<CredentialFormat> {
        Some(CredentialFormat::W3CVcDataModelV11)
    }

    #[fixture]
    pub fn creator() -> Option<String> {
        Some("Creator Name".to_string())
    }

    #[fixture]
    pub fn holder_type() -> Option<HolderType> {
        Some(HolderType::Individual)
    }

    #[fixture]
    pub fn modified_at() -> String {
        "2024-01-01T00:00:00Z".to_string()
    }

    #[fixture]
    pub fn tags() -> Vec<String> {
        vec!["tag1".to_string(), "tag2".to_string()]
    }

    #[fixture]
    pub fn status() -> Status {
        Status::Draft
    }

    #[fixture]
    pub fn require_pin_code() -> Option<bool> {
        Some(true)
    }

    #[fixture]
    pub fn visibility() -> Visibility {
        Visibility::Private
    }

    #[fixture]
    pub fn description() -> Option<String> {
        Some("Sample description".to_string())
    }

    #[fixture]
    pub fn r#type() -> Vec<String> {
        vec!["Type1".to_string(), "Type2".to_string()]
    }

    #[fixture]
    pub fn schema() -> Option<serde_json::Value> {
        Some(serde_json::json!({"key": "value"}))
    }
}
