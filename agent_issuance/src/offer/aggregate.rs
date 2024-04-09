use std::sync::Arc;

use agent_secret_manager::services::SecretManagerServices;
use agent_shared::generate_random_string;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use jsonwebtoken::{Algorithm, Header};
use oid4vc_core::authentication::subject::Subject;
use oid4vc_core::{jwt, Validator};
use oid4vci::credential_issuer::CredentialIssuer;
use oid4vci::credential_offer::{CredentialOffer, CredentialOfferParameters, Grants, PreAuthorizedCode};
use oid4vci::credential_response::{CredentialResponse, CredentialResponseType};
use oid4vci::token_request::TokenRequest;
use oid4vci::token_response::TokenResponse;
use oid4vci::VerifiableCredentialJwt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;

use crate::offer::command::OfferCommand;
use crate::offer::error::OfferError::{self, *};
use crate::offer::event::OfferEvent;
use crate::offer::services::OfferServices;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Offer {
    pub credential_ids: Vec<String>,
    pub form_url_encoded_credential_offer: String,
    pub pre_authorized_code: String,
    pub token_response: Option<TokenResponse>,
    pub access_token: String,
    pub credential_response: Option<CredentialResponse>,
}

#[async_trait]
impl Aggregate for Offer {
    type Command = OfferCommand;
    type Event = OfferEvent;
    type Error = OfferError;
    type Services = OfferServices;

    fn aggregate_type() -> String {
        "offer".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use OfferCommand::*;
        use OfferEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateCredentialOffer => {
                #[cfg(test)]
                let (pre_authorized_code, access_token) = {
                    let pre_authorized_code = tests::PRE_AUTHORIZED_CODES.lock().unwrap().pop_front().unwrap();
                    let access_token = tests::ACCESS_TOKENS.lock().unwrap().pop_front().unwrap();
                    (pre_authorized_code, access_token)
                };
                #[cfg(not(test))]
                let (pre_authorized_code, access_token) = { (generate_random_string(), generate_random_string()) };

                Ok(vec![CredentialOfferCreated {
                    pre_authorized_code,
                    access_token,
                }])
            }
            AddCredentials { credential_ids } => Ok(vec![CredentialsAdded { credential_ids }]),
            CreateFormUrlEncodedCredentialOffer {
                credential_issuer_metadata,
            } => {
                let credentials_supported = credential_issuer_metadata.credential_configurations_supported.clone();
                let credential_offer = CredentialOffer::CredentialOffer(Box::new(CredentialOfferParameters {
                    credential_issuer: credential_issuer_metadata.credential_issuer.clone(),
                    credential_configuration_ids: credentials_supported.keys().cloned().collect(),
                    grants: Some(Grants {
                        authorization_code: None,
                        pre_authorized_code: Some(PreAuthorizedCode {
                            pre_authorized_code: self.pre_authorized_code.clone(),
                            ..Default::default()
                        }),
                    }),
                }));
                Ok(vec![FormUrlEncodedCredentialOfferCreated {
                    form_url_encoded_credential_offer: credential_offer.to_string(),
                }])
            }
            CreateTokenResponse { token_request } => {
                #[cfg(test)]
                let c_nonce = tests::C_NONCES.lock().unwrap().pop_front().unwrap();
                #[cfg(not(test))]
                let c_nonce = generate_random_string();

                match token_request {
                    TokenRequest::PreAuthorizedCode { .. } => Ok(vec![TokenResponseCreated {
                        token_response: TokenResponse {
                            access_token: self.access_token.clone(),
                            token_type: "bearer".to_string(),
                            expires_in: None,
                            refresh_token: None,
                            scope: None,
                            c_nonce: Some(c_nonce),
                            c_nonce_expires_in: None,
                        },
                    }]),
                    _ => todo!(),
                }
            }
            CreateCredentialResponse {
                credential_issuer_metadata,
                authorization_server_metadata,
                mut credentials,
                credential_request,
            } => {
                // TODO: support batch credentials.
                let mut credential = credentials.pop().ok_or(MissingCredentialError)?;

                let issuer = Arc::new(futures::executor::block_on(async {
                    let mut services = SecretManagerServices::new(None);
                    services.init().await.unwrap();
                    services.secret_manager.unwrap()
                }));
                let issuer_did = issuer.identifier("did:key").unwrap();

                let credential_issuer = CredentialIssuer {
                    subject: issuer.clone(),
                    metadata: credential_issuer_metadata,
                    authorization_server_metadata: *authorization_server_metadata,
                };

                let proof = credential_issuer
                    .validate_proof(
                        credential_request.proof.ok_or(MissingProofError)?,
                        Validator::Subject(issuer.clone()),
                    )
                    .await
                    .map_err(|e| InvalidProofError(e.to_string()))?;

                let subject_did = proof
                    .rfc7519_claims
                    .iss()
                    .as_ref()
                    .ok_or(MissingProofIssuerError)?
                    .clone();

                credential.raw["issuer"] = json!(issuer_did);
                credential.raw["credentialSubject"]["id"] = json!(subject_did);
                let credential_response = CredentialResponse {
                    credential: CredentialResponseType::Immediate {
                        credential: json!(jwt::encode(
                            issuer.clone(),
                            Header::new(Algorithm::EdDSA),
                            VerifiableCredentialJwt::builder()
                                .sub(subject_did)
                                .iss(issuer_did)
                                .iat(0)
                                .exp(9999999999i64)
                                .verifiable_credential(credential.raw)
                                .build()
                                .ok(),
                            "did:key"
                        )
                        .ok()),
                        notification_id: None,
                    },
                    c_nonce: None,
                    c_nonce_expires_in: None,
                };

                Ok(vec![CredentialResponseCreated { credential_response }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use OfferEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            CredentialOfferCreated {
                pre_authorized_code,
                access_token,
            } => {
                *self = Self {
                    pre_authorized_code,
                    access_token,
                    // In case of re-creating an `Offer`, this ensures that all the other fields are reset.
                    // TODO: what to do with the `access_token` and `pre-authorize_code` views? They should be reset as well.
                    ..Default::default()
                };
            }
            CredentialsAdded { credential_ids } => {
                self.credential_ids = credential_ids;
            }
            FormUrlEncodedCredentialOfferCreated {
                form_url_encoded_credential_offer,
            } => {
                self.form_url_encoded_credential_offer = form_url_encoded_credential_offer;
            }
            TokenResponseCreated { token_response } => {
                self.token_response.replace(token_response);
            }
            CredentialResponseCreated { credential_response } => {
                self.credential_response.replace(credential_response);
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use cqrs_es::test::TestFramework;
    use did_manager::SecretManager;
    use lazy_static::lazy_static;
    use oid4vci::{
        credential_format_profiles::{
            w3c_verifiable_credentials::jwt_vc_json::CredentialDefinition, CredentialFormats, Parameters,
        },
        credential_request::CredentialRequest,
        KeyProofType, ProofType,
    };
    use std::{collections::VecDeque, sync::Mutex};

    use crate::{
        credential::entity::Data,
        server_config::aggregate::server_config_tests::{AUTHORIZATION_SERVER_METADATA, CREDENTIAL_ISSUER_METADATA},
    };

    type OfferTestFramework = TestFramework<Offer>;

    #[test]
    #[serial_test::serial]
    fn test_create_offer() {
        *PRE_AUTHORIZED_CODES.lock().unwrap() = vec![generate_random_string()].into();
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject = subject();
        OfferTestFramework::with(OfferServices)
            .given_no_previous_events()
            .when(OfferCommand::CreateCredentialOffer)
            .then_expect_events(vec![OfferEvent::CredentialOfferCreated {
                pre_authorized_code: subject.pre_authorized_code,
                access_token: subject.access_token,
            }]);
    }

    #[test]
    #[serial_test::serial]
    fn test_add_credential() {
        *PRE_AUTHORIZED_CODES.lock().unwrap() = vec![generate_random_string()].into();
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject = subject();
        OfferTestFramework::with(OfferServices)
            .given(vec![OfferEvent::CredentialOfferCreated {
                pre_authorized_code: subject.pre_authorized_code.clone(),
                access_token: subject.access_token.clone(),
            }])
            .when(OfferCommand::AddCredentials {
                credential_ids: vec!["credential-id".to_string()],
            })
            .then_expect_events(vec![OfferEvent::CredentialsAdded {
                credential_ids: vec!["credential-id".to_string()],
            }]);
    }

    #[test]
    #[serial_test::serial]
    fn test_create_credential_offer() {
        *PRE_AUTHORIZED_CODES.lock().unwrap() = vec![generate_random_string()].into();
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject = subject();
        OfferTestFramework::with(OfferServices)
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    pre_authorized_code: subject.pre_authorized_code,
                    access_token: subject.access_token,
                },
                OfferEvent::CredentialsAdded {
                    credential_ids: vec!["credential-id".to_string()],
                },
            ])
            .when(OfferCommand::CreateFormUrlEncodedCredentialOffer {
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            })
            .then_expect_events(vec![OfferEvent::FormUrlEncodedCredentialOfferCreated {
                form_url_encoded_credential_offer: subject.form_url_encoded_credential_offer,
            }]);
    }

    #[test]
    #[serial_test::serial]
    fn test_create_token_response() {
        *PRE_AUTHORIZED_CODES.lock().unwrap() = vec![generate_random_string()].into();
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject = subject();
        OfferTestFramework::with(OfferServices)
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    pre_authorized_code: subject.pre_authorized_code.clone(),
                    access_token: subject.access_token.clone(),
                },
                OfferEvent::CredentialsAdded {
                    credential_ids: vec!["credential-id".to_string()],
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    form_url_encoded_credential_offer: subject.form_url_encoded_credential_offer.clone(),
                },
            ])
            .when(OfferCommand::CreateTokenResponse {
                token_request: token_request(subject.clone()),
            })
            .then_expect_events(vec![OfferEvent::TokenResponseCreated {
                token_response: token_response(subject),
            }]);
    }

    #[test]
    #[serial_test::serial]
    fn test_create_credential_response() {
        *PRE_AUTHORIZED_CODES.lock().unwrap() = vec![generate_random_string()].into();
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject = subject();
        OfferTestFramework::with(OfferServices)
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    pre_authorized_code: subject.pre_authorized_code.clone(),
                    access_token: subject.access_token.clone(),
                },
                OfferEvent::CredentialsAdded {
                    credential_ids: vec!["credential-id".to_string()],
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    form_url_encoded_credential_offer: subject.form_url_encoded_credential_offer.clone(),
                },
                OfferEvent::TokenResponseCreated {
                    token_response: token_response(subject.clone()),
                },
            ])
            .when(OfferCommand::CreateCredentialResponse {
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
                authorization_server_metadata: AUTHORIZATION_SERVER_METADATA.clone(),
                credentials: vec![UNSIGNED_CREDENTIAL.clone()],
                credential_request: credential_request(subject.clone()),
            })
            .then_expect_events(vec![OfferEvent::CredentialResponseCreated {
                credential_response: credential_response(subject),
            }]);
    }

    #[test]
    #[serial_test::serial]
    fn test_create_credential_response_multiple_subjects() {
        *PRE_AUTHORIZED_CODES.lock().unwrap() = vec![generate_random_string()].into();
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject = subject();
        OfferTestFramework::with(OfferServices)
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    pre_authorized_code: subject.pre_authorized_code.clone(),
                    access_token: subject.access_token.clone(),
                },
                OfferEvent::CredentialsAdded {
                    credential_ids: vec!["credential-id".to_string()],
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    form_url_encoded_credential_offer: subject.form_url_encoded_credential_offer.clone(),
                },
                OfferEvent::TokenResponseCreated {
                    token_response: token_response(subject.clone()),
                },
            ])
            .when(OfferCommand::CreateCredentialResponse {
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
                authorization_server_metadata: AUTHORIZATION_SERVER_METADATA.clone(),
                credentials: vec![UNSIGNED_CREDENTIAL.clone()],
                credential_request: credential_request(subject.clone()),
            })
            .then_expect_events(vec![OfferEvent::CredentialResponseCreated {
                credential_response: credential_response(subject),
            }]);
    }

    #[derive(Clone)]
    struct TestSubject {
        secret_manager: Arc<SecretManager>,
        credential: String,
        access_token: String,
        pre_authorized_code: String,
        form_url_encoded_credential_offer: String,
        c_nonce: String,
    }

    lazy_static! {
        pub static ref PRE_AUTHORIZED_CODES: Mutex<VecDeque<String>> = Mutex::new(vec![].into());
        pub static ref ACCESS_TOKENS: Mutex<VecDeque<String>> = Mutex::new(vec![].into());
        pub static ref C_NONCES: Mutex<VecDeque<String>> = Mutex::new(vec![].into());
        static ref BASE_URL: url::Url = "https://example.com/".parse().unwrap();
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
        static ref UNSIGNED_CREDENTIAL: Data = Data {
            raw: json!({
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
            })
        };
        static ref SUBJECT_1_KEY_DID: Arc<SecretManager> = Arc::new(secret_manager());
        static ref SUBJECT_2_KEY_DID: Arc<SecretManager> = Arc::new(secret_manager());
        static ref VERIFIABLE_CREDENTIAL_JWT_1: String = {
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
        static ref VERIFIABLE_CREDENTIAL_JWT_2: String = {
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa3F5WmpEZmhzeVo1YzZOdUpoYm9zV2tTajg2Mmp5V2lDQ\
            0tIRHpOTkttOGtoI3o2TWtxeVpqRGZoc3laNWM2TnVKaGJvc1drU2o4NjJqeVdpQ0NLSER6Tk5LbThraCJ9.eyJpc3MiOiJkaWQ6a2V5On\
            o2TWtxeVpqRGZoc3laNWM2TnVKaGJvc1drU2o4NjJqeVdpQ0NLSER6Tk5LbThraCIsInN1YiI6ImRpZDprZXk6ejZNa3ZrNVptb2dXa042\
            RmtQRXJaVW1uVlNiQ0tmdEdCVjhqcGJ6UFZmeG8zdERBIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjpbIm\
            h0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9wdXJsLmltc2dsb2JhbC5vcmcvc3BlYy9vYi92M3Aw\
            L2NvbnRleHQtMy4wLjIuanNvbiJdLCJpZCI6Imh0dHA6Ly9leGFtcGxlLmNvbS9jcmVkZW50aWFscy8zNTI3IiwidHlwZSI6WyJWZXJpZm\
            lhYmxlQ3JlZGVudGlhbCIsIk9wZW5CYWRnZUNyZWRlbnRpYWwiXSwiaXNzdWVyIjoiZGlkOmtleTp6Nk1rcXlaakRmaHN5WjVjNk51Smhi\
            b3NXa1NqODYyanlXaUNDS0hEek5OS204a2giLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsIm5hbWUiOiJUZWFtd2\
            9yayBCYWRnZSIsImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rdms1Wm1vZ1drTjZGa1BFclpVbW5WU2JDS2Z0R0JW\
            OGpwYnpQVmZ4bzN0REEiLCJ0eXBlIjoiQWNoaWV2ZW1lbnRTdWJqZWN0IiwiYWNoaWV2ZW1lbnQiOnsiaWQiOiJodHRwczovL2V4YW1wbG\
            UuY29tL2FjaGlldmVtZW50cy8yMXN0LWNlbnR1cnktc2tpbGxzL3RlYW13b3JrIiwidHlwZSI6IkFjaGlldmVtZW50IiwiY3JpdGVyaWEi\
            OnsibmFycmF0aXZlIjoiVGVhbSBtZW1iZXJzIGFyZSBub21pbmF0ZWQgZm9yIHRoaXMgYmFkZ2UgYnkgdGhlaXIgcGVlcnMgYW5kIHJlY2\
            9nbml6ZWQgdXBvbiByZXZpZXcgYnkgRXhhbXBsZSBDb3JwIG1hbmFnZW1lbnQuIn0sImRlc2NyaXB0aW9uIjoiVGhpcyBiYWRnZSByZWNv\
            Z25pemVzIHRoZSBkZXZlbG9wbWVudCBvZiB0aGUgY2FwYWNpdHkgdG8gY29sbGFib3JhdGUgd2l0aGluIGEgZ3JvdXAgZW52aXJvbm1lbn\
            QuIiwibmFtZSI6IlRlYW13b3JrIn19fX0.X4Hp_bZTfc54c_PcBI2-Qr3YjEZbUB72t4ppo-URRQKDjQXhZmzeox1pMBGQ-hjbNVmdvixw\
            kdSvpwvZnx6zAg"
                .to_string()
        };
    }

    fn subject() -> TestSubject {
        let pre_authorized_code = PRE_AUTHORIZED_CODES.lock().unwrap()[0].clone();

        TestSubject {
            secret_manager: SUBJECT_1_KEY_DID.clone(),
            credential: VERIFIABLE_CREDENTIAL_JWT_1.clone(),
            pre_authorized_code: pre_authorized_code.clone(),
            access_token: ACCESS_TOKENS.lock().unwrap()[0].clone(),
            form_url_encoded_credential_offer: format!("openid-credential-offer://?credential_offer=%7B%22credential_issuer%22%3A%22https%3A%2F%2Fexample.com%2F%22%2C%22credential_configuration_ids%22%3A%5B%220%22%5D%2C%22grants%22%3A%7B%22urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Apre-authorized_code%22%3A%7B%22pre-authorized_code%22%3A%22{pre_authorized_code}%22%7D%7D%7D"),
            c_nonce: C_NONCES.lock().unwrap()[0].clone(),
        }
    }

    fn token_request(subject: TestSubject) -> TokenRequest {
        TokenRequest::PreAuthorizedCode {
            pre_authorized_code: subject.pre_authorized_code,
            tx_code: None,
        }
    }

    fn token_response(subject: TestSubject) -> TokenResponse {
        TokenResponse {
            access_token: subject.access_token.clone(),
            token_type: "bearer".to_string(),
            expires_in: None,
            refresh_token: None,
            scope: None,
            c_nonce: Some(subject.c_nonce.clone()),
            c_nonce_expires_in: None,
        }
    }

    fn credential_request(subject: TestSubject) -> CredentialRequest {
        CredentialRequest {
            credential_format: CredentialFormats::JwtVcJson(Parameters {
                parameters: (
                    CredentialDefinition {
                        type_: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
                        credential_subject: Default::default(),
                    },
                    None,
                )
                    .into(),
            }),
            proof: Some(
                KeyProofType::builder()
                    .proof_type(ProofType::Jwt)
                    .signer(subject.secret_manager.clone())
                    .iss(subject.secret_manager.identifier("did:key").unwrap())
                    .aud(CREDENTIAL_ISSUER_METADATA.credential_issuer.clone())
                    .iat(1571324800)
                    .exp(9999999999i64)
                    .nonce(subject.c_nonce.clone())
                    .subject_syntax_type("did:key")
                    .build()
                    .unwrap(),
            ),
        }
    }

    fn credential_response(subject: TestSubject) -> CredentialResponse {
        CredentialResponse {
            credential: CredentialResponseType::Immediate {
                credential: json!(subject.credential.clone()),
                notification_id: None,
            },
            c_nonce: None,
            c_nonce_expires_in: None,
        }
    }

    fn secret_manager() -> SecretManager {
        futures::executor::block_on(async {
            let mut services = SecretManagerServices::new(None);
            services.init().await.unwrap();
            services.secret_manager.unwrap()
        })
    }
}
