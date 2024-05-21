use agent_secret_manager::services::SecretManagerServices;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use jsonwebtoken::{Algorithm, Header};
use oid4vc_core::{jwt, Subject as _};
use oid4vci::VerifiableCredentialJwt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::info;

use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError::{self, InvalidCredentialError};
use crate::credential::event::CredentialEvent;
use crate::credential::services::CredentialServices;

use super::entity::Data;

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct Credential {
    data: Option<Data>,
    credential_format_template: Option<serde_json::Value>,
    signed: Option<serde_json::Value>,
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
        use CredentialCommand::*;
        use CredentialEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateUnsignedCredential {
                data,
                credential_format_template,
            } => {
                let mut events = vec![];

                let mut unsigned_credential = credential_format_template.clone();

                unsigned_credential
                    .as_object_mut()
                    .ok_or(InvalidCredentialError)?
                    .insert("credentialSubject".to_string(), data.raw["credentialSubject"].clone());

                events.push(UnsignedCredentialCreated {
                    data: Data {
                        raw: unsigned_credential.clone(),
                    },
                    credential_format_template,
                });

                Ok(events)
            }
            CreateSignedCredential { signed_credential } => Ok(vec![SignedCredentialCreated { signed_credential }]),
            SignCredential { subject_id, overwrite } => {
                if self.signed.is_some() && !overwrite {
                    return Ok(vec![]);
                }
                let (issuer, default_did_method) = {
                    let mut services = SecretManagerServices::new(None);
                    services.init().await.unwrap();
                    (Arc::new(services.subject.unwrap()), services.default_did_method.clone())
                };
                let issuer_did = issuer.identifier(&default_did_method, Algorithm::EdDSA).await.unwrap();
                let signed_credential = {
                    // TODO: Add error message here.
                    let mut credential = self.data.clone().unwrap();

                    credential.raw["issuer"] = json!(issuer_did);
                    credential.raw["credentialSubject"]["id"] = json!(subject_id);

                    json!(jwt::encode(
                        issuer.clone(),
                        Header::new(Algorithm::EdDSA),
                        VerifiableCredentialJwt::builder()
                            .sub(subject_id)
                            .iss(issuer_did)
                            .iat(0)
                            .exp(9999999999i64)
                            .verifiable_credential(credential.raw)
                            .build()
                            .ok(),
                        &default_did_method
                    )
                    .await
                    .ok())
                };

                Ok(vec![CredentialSigned { signed_credential }])
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
                self.data.replace(data);
                self.credential_format_template.replace(credential_format_template);
            }
            SignedCredentialCreated { signed_credential } => {
                self.signed.replace(signed_credential);
            }
            CredentialSigned { signed_credential } => {
                self.signed.replace(signed_credential);
            }
        }
    }
}

#[cfg(test)]
pub mod credential_tests {
    use super::*;

    use lazy_static::lazy_static;
    use rstest::rstest;
    use serde_json::json;

    use cqrs_es::test::TestFramework;

    use crate::credential::aggregate::Credential;
    use crate::credential::event::CredentialEvent;
    use crate::offer::aggregate::tests::SUBJECT_KEY_DID;

    type CredentialTestFramework = TestFramework<Credential>;

    #[rstest]
    #[serial_test::serial]
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

    #[rstest]
    #[serial_test::serial]
    async fn test_sign_credential() {
        CredentialTestFramework::with(CredentialServices)
            .given(vec![CredentialEvent::UnsignedCredentialCreated {
                data: Data {
                    raw: UNSIGNED_CREDENTIAL.clone(),
                },
                credential_format_template: CREDENTIAL_FORMAT_TEMPLATE.clone(),
            }])
            .when(CredentialCommand::SignCredential {
                subject_id: SUBJECT_KEY_DID.identifier("did:key", Algorithm::EdDSA).await.unwrap(),
                overwrite: false,
            })
            .then_expect_events(vec![CredentialEvent::CredentialSigned {
                signed_credential: json!(VERIFIABLE_CREDENTIAL_JWT.clone()),
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
        pub static ref VERIFIABLE_CREDENTIAL_JWT: String = {
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dF\
             ODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtn\
             RTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJ\
             pc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXd\
             KdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDl\
             qSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwiZXhwIjo5OTk5OTk5OTk\
             5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8\
             yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9wdXJsLmltc2dsb2JhbC5vcmc\
             vc3BlYy9vYi92M3AwL2NvbnRleHQtMy4wLjIuanNvbiJdLCJpZCI6Imh0dHA6Ly9\
             leGFtcGxlLmNvbS9jcmVkZW50aWFscy8zNTI3IiwidHlwZSI6WyJWZXJpZmlhYmx\
             lQ3JlZGVudGlhbCIsIk9wZW5CYWRnZUNyZWRlbnRpYWwiXSwiaXNzdWVyIjoiZGl\
             kOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05\
             rOEtDZ3QiLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsIm5\
             hbWUiOiJUZWFtd29yayBCYWRnZSIsImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjo\
             iZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU\
             3d05rOEtDZ3QiLCJ0eXBlIjoiQWNoaWV2ZW1lbnRTdWJqZWN0IiwiYWNoaWV2ZW1\
             lbnQiOnsiaWQiOiJodHRwczovL2V4YW1wbGUuY29tL2FjaGlldmVtZW50cy8yMXN\
             0LWNlbnR1cnktc2tpbGxzL3RlYW13b3JrIiwidHlwZSI6IkFjaGlldmVtZW50Iiw\
             iY3JpdGVyaWEiOnsibmFycmF0aXZlIjoiVGVhbSBtZW1iZXJzIGFyZSBub21pbmF\
             0ZWQgZm9yIHRoaXMgYmFkZ2UgYnkgdGhlaXIgcGVlcnMgYW5kIHJlY29nbml6ZWQ\
             gdXBvbiByZXZpZXcgYnkgRXhhbXBsZSBDb3JwIG1hbmFnZW1lbnQuIn0sImRlc2N\
             yaXB0aW9uIjoiVGhpcyBiYWRnZSByZWNvZ25pemVzIHRoZSBkZXZlbG9wbWVudCB\
             vZiB0aGUgY2FwYWNpdHkgdG8gY29sbGFib3JhdGUgd2l0aGluIGEgZ3JvdXAgZW5\
             2aXJvbm1lbnQuIiwibmFtZSI6IlRlYW13b3JrIn19fX0.meErQE7y_AnGa4le_8L\
             8FVhIQDdTUPBVHdeW8Q4UGU3BeeT3O-7OeagveXh4_5aZEYj-eoiaR_JNgAkpzjd\
             pCA"
            .to_string()
        };
    }
}
