use async_trait::async_trait;
use cqrs_es::Aggregate;
use jsonschema::JSONSchema;
use serde::{Deserialize, Serialize};

use crate::{
    command::IssuanceCommand, error::IssuanceError, event::IssuanceEvent,
    services::IssuanceServices,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CredentialTemplate {
    // json_schema
    metadata_schema: serde_json::Value,
    // json_schema
    subject_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Credential {
    credential_template: CredentialTemplate,
    credential_data: serde_json::Value,
    // TODO: add proof?
    // proof: Option<T>
}

#[async_trait]
impl Aggregate for Credential {
    type Command = IssuanceCommand;
    type Event = IssuanceEvent;
    type Error = IssuanceError;
    type Services = IssuanceServices;

    fn aggregate_type() -> String {
        "Credential".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            IssuanceCommand::LoadCredentialTemplate(credential_template) => {
                JSONSchema::compile(&credential_template)
                    .map_err(|e| IssuanceError::from(e.to_string().as_str()))?;

                let credential_template = CredentialTemplate {
                    metadata_schema: credential_template,
                    subject_schema: None,
                };

                Ok(vec![IssuanceEvent::CredentialTemplateLoaded {
                    credential_template,
                }])
            }
            IssuanceCommand::CreateCredentialData {
                credential_subject,
                metadata,
            } => {
                let credential_template = CredentialTemplate {
                    metadata_schema: serde_json::json!(metadata),
                    subject_schema: Some(credential_subject),
                };
                let credential_data = serde_json::json!({});
                Ok(vec![IssuanceEvent::CredentialDataCreated {
                    credential_template,
                    credential_data,
                }])
            }
            _ => unimplemented!(),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use IssuanceEvent::*;
        match event {
            CredentialTemplateLoaded {
                credential_template,
            } => self.credential_template = credential_template,
            CredentialDataCreated {
                credential_template,
                credential_data,
            } => {
                self.credential_template = credential_template;
                self.credential_data = credential_data;
            }
            CredentialSigned { .. } => todo!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Metadata;
    use cqrs_es::test::TestFramework;

    type CredentialTestFramework = TestFramework<Credential>;

    #[test]
    fn test_credential_template_loaded() {
        let expected = IssuanceEvent::CredentialTemplateLoaded {
            credential_template: CredentialTemplate {
                metadata_schema: serde_json::json!({"foo": "bar"}),
                subject_schema: None,
            },
        };

        CredentialTestFramework::with(IssuanceServices)
            .given_no_previous_events()
            .when(IssuanceCommand::LoadCredentialTemplate(
                serde_json::json!({"foo": "bar"}),
            ))
            .then_expect_events(vec![expected]);
    }

    #[test]
    fn test_create_data_created() {
        let expected = IssuanceEvent::CredentialDataCreated {
            credential_template: CredentialTemplate {
                metadata_schema: serde_json::json!({"foo": "bar"}),
                subject_schema: None,
            },
            credential_data: serde_json::json!({}),
        };

        CredentialTestFramework::with(IssuanceServices)
            .given(vec![IssuanceEvent::CredentialTemplateLoaded {
                credential_template: CredentialTemplate {
                    metadata_schema: serde_json::json!({"foo": "bar"}),
                    subject_schema: None,
                },
            }])
            .when(IssuanceCommand::CreateCredentialData {
                credential_subject: serde_json::json!({}),
                metadata: Metadata {
                    credential_type: vec![],
                },
            })
            .then_expect_events(vec![expected]);
    }
}
