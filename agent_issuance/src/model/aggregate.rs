use async_trait::async_trait;
use cqrs_es::Aggregate;
use jsonschema::JSONSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{command::IssuanceCommand, error::IssuanceError, event::IssuanceEvent, services::IssuanceServices};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Credential {
    credential_template: serde_json::Value,
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
            IssuanceCommand::LoadCredentialTemplate { credential_template } => {
                JSONSchema::compile(&credential_template).map_err(|e| IssuanceError::from(e.to_string().as_str()))?;

                Ok(vec![IssuanceEvent::CredentialTemplateLoaded { credential_template }])
            }
            IssuanceCommand::CreateCredentialData { credential_subject } => {
                let credential_template = self.credential_template.clone();
                dbg!(&credential_template);
                // let json_schema = JSONSchema::compile(&credential_template)
                //     .map_err(|e| IssuanceError::from(e.to_string().as_str()))?;

                let mut openbadges_v3_format_template =
                    serde_json::from_str::<Value>(include_str!("../../res/format_templates/openbadges_v3.json"))
                        .map_err(|e| IssuanceError::from(e.to_string().as_str()))?;

                openbadges_v3_format_template
                    .as_object_mut()
                    .unwrap()
                    .insert("credentialSubject".to_string(), credential_subject.clone());

                dbg!(&openbadges_v3_format_template);

                // json_schema.validate(&openbadges_v3_format_template).map_err(|e| {
                //     // TODO: remove ugly solution.
                //     let e: Vec<_> = e.map(|e| e.to_string()).collect();
                //     IssuanceError::from(e.join(", ").as_str())
                // })?;

                Ok(vec![IssuanceEvent::CredentialDataCreated {
                    credential_template,
                    credential_data: credential_subject,
                }])
            }
            _ => unimplemented!(),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use IssuanceEvent::*;
        match event {
            CredentialTemplateLoaded { credential_template } => self.credential_template = credential_template,
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
    use cqrs_es::test::TestFramework;
    use serde_json::json;

    type CredentialTestFramework = TestFramework<Credential>;

    pub fn credential_template() -> serde_json::Value {
        serde_json::from_str(include_str!("../../res/json_schema/openbadges_v3.json")).unwrap()
    }

    #[test]
    fn test_credential_template_loaded() {
        let expected = IssuanceEvent::CredentialTemplateLoaded {
            credential_template: credential_template(),
        };

        CredentialTestFramework::with(IssuanceServices)
            .given_no_previous_events()
            .when(IssuanceCommand::LoadCredentialTemplate {
                credential_template: credential_template(),
            })
            .then_expect_events(vec![expected]);
    }

    #[test]
    fn test_create_data_created() {
        let credential = json!({
          "credentialSubject": {
            "id": "did:example:ebfeb1f712ebc6f1c276e12ec21",
            "type": "AchievementSubject",
            "achievement": {
                      "id": "https://example.com/achievements/21st-century-skills/teamwork",
                      "type": "Achievement",
                      "criteria": {
                          "narrative": "Team members are nominated for this badge by their peers and recognized upon review by Example Corp management."
                      },
                      "description": "This badge recognizes the development of the capacity to collaborate within a group environment.",
                      "name": "Teamwork"
                  }
          }
        });

        let expected = IssuanceEvent::CredentialDataCreated {
            credential_template: credential_template(),
            credential_data: credential.clone(),
        };

        CredentialTestFramework::with(IssuanceServices)
            .given(vec![IssuanceEvent::CredentialTemplateLoaded {
                credential_template: credential_template(),
            }])
            .when(IssuanceCommand::CreateCredentialData {
                credential_subject: credential,
            })
            .then_expect_events(vec![expected]);
    }
}
