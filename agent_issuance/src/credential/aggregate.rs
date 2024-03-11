use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError::{self, InvalidCredentialError};
use crate::credential::event::CredentialEvent;
use crate::credential::services::CredentialServices;

use super::entity::Data;

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct Credential {
    pub data: Data,
    pub credential_format_template: serde_json::Value,
}

#[async_trait]
impl Aggregate for Credential {
    type Command = CredentialCommand;
    type Event = CredentialEvent;
    type Error = CredentialError;
    type Services = CredentialServices;

    fn aggregate_type() -> String {
        "credential".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        info!("Handling command: {:?}", command);

        match command {
            CredentialCommand::CreateUnsignedCredential {
                data,
                credential_format_template,
            } => {
                let mut events = vec![];

                let mut unsigned_credential = credential_format_template.clone();

                unsigned_credential
                    .as_object_mut()
                    .ok_or(InvalidCredentialError)?
                    .insert("credentialSubject".to_string(), data.raw["credentialSubject"].clone());

                events.push(CredentialEvent::UnsignedCredentialCreated {
                    data: Data {
                        raw: unsigned_credential.clone(),
                    },
                    credential_format_template,
                });

                Ok(events)
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use CredentialEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            UnsignedCredentialCreated {
                data,
                credential_format_template,
            } => {
                self.data = data;
                self.credential_format_template = credential_format_template;
            }
        }
    }
}

#[cfg(test)]
pub mod credential_tests {
    use super::*;

    use lazy_static::lazy_static;
    use serde_json::json;

    use cqrs_es::test::TestFramework;

    use crate::credential::aggregate::Credential;
    use crate::credential::event::CredentialEvent;

    type CredentialTestFramework = TestFramework<Credential>;

    #[test]
    fn test_create_unsigned_credential() {
        CredentialTestFramework::with(CredentialServices)
            .given_no_previous_events()
            .when(CredentialCommand::CreateUnsignedCredential {
                data: Data {
                    raw: CREDENTIAL_SUBJECT.clone(),
                },
                credential_format_template: CREDENTIAL_FORMAT_TEMPLATE.clone(),
            })
            .then_expect_events(vec![CredentialEvent::UnsignedCredentialCreated {
                data: Data {
                    raw: UNSIGNED_CREDENTIAL.clone(),
                },
                credential_format_template: CREDENTIAL_FORMAT_TEMPLATE.clone(),
            }])
    }

    lazy_static! {
        static ref CREDENTIAL_FORMAT_TEMPLATE: serde_json::Value =
            serde_json::from_str(include_str!("../../res/credential_format_templates/openbadges_v3.json")).unwrap();
        static ref CREDENTIAL_SUBJECT: serde_json::Value = json!(
            {
                "credentialSubject": {
                    "id": {},
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
            }
        );
        static ref UNSIGNED_CREDENTIAL: serde_json::Value = json!({
          "@context": [
            "https://www.w3.org/2018/credentials/v1",
            "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.2.json"
          ],
          "id": "http://example.com/credentials/3527",
          "type": ["VerifiableCredential", "OpenBadgeCredential"],
          "issuer": {
            "id": "https://example.com/issuers/876543",
            "type": "Profile",
            "name": "Example Corp"
          },
          "issuanceDate": "2010-01-01T00:00:00Z",
          "name": "Teamwork Badge",
          "credentialSubject": CREDENTIAL_SUBJECT["credentialSubject"].clone(),
        });
    }
}
