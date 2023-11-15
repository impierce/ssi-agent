use async_trait::async_trait;
use cqrs_es::Aggregate;
use jsonschema::JSONSchema;
use serde::{Deserialize, Serialize};

use crate::{
    command::IssuanceCommand, error::IssuanceError, event::IssuanceEvent,
    services::IssuanceServices,
};

// #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
// pub struct CredentialTemplate {
//     // json_schema
//     credential_template: serde_json::Value,
//     // json_schema
//     subject_schema: Option<serde_json::Value>,
// }

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
            IssuanceCommand::LoadCredentialTemplate(credential_template) => {
                JSONSchema::compile(&credential_template)
                    .map_err(|e| IssuanceError::from(e.to_string().as_str()))?;

                Ok(vec![IssuanceEvent::CredentialTemplateLoaded {
                    credential_template,
                }])
            }
            IssuanceCommand::CreateCredentialData { credential } => {
                let credential_template = self.credential_template.clone();
                let json_schema = JSONSchema::compile(&credential_template)
                    .map_err(|e| IssuanceError::from(e.to_string().as_str()))?;

                json_schema.validate(&credential).map_err(|e| {
                    // TODO: remove ugly solution.
                    let e: Vec<_> = e.map(|e| e.to_string()).collect();
                    IssuanceError::from(e.join(", ").as_str())
                })?;

                Ok(vec![IssuanceEvent::CredentialDataCreated {
                    credential_template,
                    credential_data: credential,
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
    use identity_credential::credential;
    use serde_json::json;

    type CredentialTestFramework = TestFramework<Credential>;

    fn credential_template() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../resources/json_schema/w3c_vc_data_model_v2.json"
        ))
        .unwrap()
    }

    #[test]
    fn test_credential_template_loaded() {
        let expected = IssuanceEvent::CredentialTemplateLoaded {
            credential_template: credential_template(),
        };

        CredentialTestFramework::with(IssuanceServices)
            .given_no_previous_events()
            .when(IssuanceCommand::LoadCredentialTemplate(
                credential_template(),
            ))
            .then_expect_events(vec![expected]);
    }

    #[test]
    fn test_create_data_created() {
        let credential = json!({
        "@context": [
            "https://www.w3.org/ns/credentials/v2",
            "https://www.w3.org/ns/credentials/examples/v2"
        ],
        "type": ["VerifiableCredential", "UniversityDegreeCredential"],
        "credentialSubject": {
          "id": "did:example:123",
          "degree": {
            "type": "BachelorDegree",
            "name": "Bachelor of Science",
            "college": "Example University"
          }
        },
        "issuanceDate": "2023-01-01T00:00:00Z",
        "issuer": "did:example:456",
        "proof": {
          "type": "Ed25519Signature2018",
          "created": "2023-01-01T00:00:00Z",
          "proofPurpose": "assertionMethod",
          "verificationMethod": "did:example:456#key1",
          "jws": "..."
        }});

        let expected = IssuanceEvent::CredentialDataCreated {
            credential_template: credential_template(),
            credential_data: credential.clone(),
        };

        CredentialTestFramework::with(IssuanceServices)
            .given(vec![IssuanceEvent::CredentialTemplateLoaded {
                credential_template: credential_template(),
            }])
            .when(IssuanceCommand::CreateCredentialData { credential })
            .then_expect_events(vec![expected]);
    }
}
