use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::credential::command::CredentialCommand;
use crate::credential::entity::Data;
use crate::credential::error::CredentialError::{self, InvalidCredentialError};
use crate::credential::event::CredentialEvent;
use crate::credential::services::CredentialServices;
use crate::credential::value_object::Subject;

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct Credential {
    // Entity
    pub data: Data,
    // Value Objects
    pub credential_format_template: serde_json::Value,
    pub subject: Subject,
}

// #[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
// #[derivative(PartialEq)]
// pub struct IssuanceSubject {
//     #[derivative(PartialEq = "ignore")]
//     pub id: String,
//     pub credential_offer: Option<CredentialOffer>,
//     pub credentials: Option<Credential>,
//     pub pre_authorized_code: String,
//     pub token_response: Option<TokenResponse>,
//     pub credential_response: Option<CredentialResponse>,
// }

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
        match command {
            CredentialCommand::LoadCredentialFormatTemplate {
                credential_format_template,
            } => {
                // self.credential_format_template = credential_format_template;
                Ok(vec![CredentialEvent::CredentialFormatTemplateLoaded {
                    credential_format_template,
                }])
            }
            CredentialCommand::CreateUnsignedCredential { subject, credential } => {
                let mut events = vec![];

                events.push(CredentialEvent::SubjectCreated {
                    subject: subject.clone(),
                });

                // if !self.subjects.iter().any(|subject| subject.id == subject_id) {
                //     events.push(CredentialEvent::SubjectCreated {
                //         // subject: IssuanceSubject {
                //         //     id: subject_id.clone(),
                //         //     pre_authorized_code: generate_random_string(),
                //         //     ..Default::default()
                //         // },
                //         subject: Subject {
                //             pre_authorized_code: "rand_str_123".to_string(),
                //         },
                //     });
                // }

                let mut unsigned_credential = self.credential_format_template.clone();

                unsigned_credential
                    .as_object_mut()
                    .ok_or(InvalidCredentialError)?
                    .insert("credentialSubject".to_string(), credential["credentialSubject"].clone());

                events.push(CredentialEvent::UnsignedCredentialCreated {
                    // subject_id,
                    credential: Credential {
                        data: Data {
                            id: uuid::Uuid::new_v4(),
                            raw: credential,
                        },
                        subject,
                        credential_format_template: unsigned_credential,
                        // id: uuid::Uuid::new_v4(),
                        // unsigned_credential,
                    },
                });

                Ok(events)
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use CredentialEvent::*;

        match event {
            CredentialFormatTemplateLoaded {
                credential_format_template,
            } => self.credential_format_template = credential_format_template,
            SubjectCreated { subject } => self.subject = subject,
            UnsignedCredentialCreated { credential } => {
                // if let Some(subject) = self.subjects.iter_mut().find(|subject| subject.id == subject_id) {
                //     subject.credentials.replace(credential);
                // }
                // self.data = Data { id: credential.data.id}
                self.data = credential.data;
                self.subject = credential.subject;
                self.credential_format_template = credential.credential_format_template;
            }
        }
    }
}
