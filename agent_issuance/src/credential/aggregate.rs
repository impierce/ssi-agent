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
                let issuer_did = issuer.identifier(&default_did_method).unwrap();
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
    use serde_json::json;

    use cqrs_es::test::TestFramework;

    use crate::credential::aggregate::Credential;
    use crate::credential::event::CredentialEvent;
    use crate::offer::aggregate::tests::SUBJECT_IDENTIFIER_KEY_ID;

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

    #[test]
    fn test_sign_credential() {
        CredentialTestFramework::with(CredentialServices)
            .given(vec![CredentialEvent::UnsignedCredentialCreated {
                data: Data {
                    raw: UNSIGNED_CREDENTIAL.clone(),
                },
                credential_format_template: CREDENTIAL_FORMAT_TEMPLATE.clone(),
            }])
            .when(CredentialCommand::SignCredential {
                subject_id: SUBJECT_IDENTIFIER_KEY_ID.clone(),
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
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGO\
            GtiY3JqWnBYM3FkI3o2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCJ9.eyJpc3MiOiJkaWQ6a2V5On\
            o2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCIsInN1YiI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNK\
            QVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjpbIm\
            h0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9wdXJsLmltc2dsb2JhbC5vcmcvc3BlYy9vYi92M3Aw\
            L2NvbnRleHQtMy4wLjIuanNvbiJdLCJpZCI6Imh0dHA6Ly9leGFtcGxlLmNvbS9jcmVkZW50aWFscy8zNTI3IiwidHlwZSI6WyJWZXJpZm\
            lhYmxlQ3JlZGVudGlhbCIsIk9wZW5CYWRnZUNyZWRlbnRpYWwiXSwiaXNzdWVyIjoiZGlkOmtleTp6Nk1raWlleW9MTVNWc0pBWnY3Smpl\
            NXdXU2tERXltVWdreUY4a2JjcmpacFgzcWQiLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsIm5hbWUiOiJUZWFtd2\
            9yayBCYWRnZSIsImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1raWlleW9MTVNWc0pBWnY3SmplNXdXU2tERXltVWdr\
            eUY4a2JjcmpacFgzcWQiLCJ0eXBlIjoiQWNoaWV2ZW1lbnRTdWJqZWN0IiwiYWNoaWV2ZW1lbnQiOnsiaWQiOiJodHRwczovL2V4YW1wbG\
            UuY29tL2FjaGlldmVtZW50cy8yMXN0LWNlbnR1cnktc2tpbGxzL3RlYW13b3JrIiwidHlwZSI6IkFjaGlldmVtZW50IiwiY3JpdGVyaWEi\
            OnsibmFycmF0aXZlIjoiVGVhbSBtZW1iZXJzIGFyZSBub21pbmF0ZWQgZm9yIHRoaXMgYmFkZ2UgYnkgdGhlaXIgcGVlcnMgYW5kIHJlY2\
            9nbml6ZWQgdXBvbiByZXZpZXcgYnkgRXhhbXBsZSBDb3JwIG1hbmFnZW1lbnQuIn0sImRlc2NyaXB0aW9uIjoiVGhpcyBiYWRnZSByZWNv\
            Z25pemVzIHRoZSBkZXZlbG9wbWVudCBvZiB0aGUgY2FwYWNpdHkgdG8gY29sbGFib3JhdGUgd2l0aGluIGEgZ3JvdXAgZW52aXJvbm1lbn\
            QuIiwibmFtZSI6IlRlYW13b3JrIn19fX0.ynkpX-rZlw0S4Vgnffn8y8fZhVOIqVid8yEUCMUNT20EC143uOMtuvpmktu5NvhXlLZTaNPe\
            _cLt0BYnPMcKDg"
                .to_string()
        };
    }
}
